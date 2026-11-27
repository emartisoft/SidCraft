// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2026 Emarti
//
// Portions derived from crabsid by Mikael Lund,
// Copyright (c) 2026 Mikael Lund, licensed under the MIT License.
// See licenses/MIT-crabsid.txt for the original license text.

use crate::engine::memory::C64Memory;
use crate::engine::sid_file::SidFile;
use mos6502::cpu::CPU;
use mos6502::instruction::Nmos6502;
use mos6502::memory::{
    Bus, IRQ_INTERRUPT_VECTOR_HI, IRQ_INTERRUPT_VECTOR_LO, NMI_INTERRUPT_VECTOR_HI,
    NMI_INTERRUPT_VECTOR_LO,
};
use mos6502::registers::{StackPointer, Status};
pub use residfp::SamplingMethod;
use residfp::{ChipModel, clock};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::{error, fmt};
const PAL_FRAME_CYCLES: u32 = 19_656;
const NTSC_FRAME_CYCLES: u32 = 17_045;

/// Ring buffer size for oscilloscope display (~23ms at 44.1kHz)
const SCOPE_BUFFER_SIZE: usize = 1024;
/// Ring buffer of mixed output samples for the spectrum and oscilloscope.
const ANALYSIS_BUFFER_SIZE: usize = 2048;
/// Piano-roll history length (about five seconds at 100 columns/s).
const NOTE_HISTORY_SIZE: usize = 512;
/// Envelope sampling divisor (sample envelope every N audio samples)
const ENVELOPE_SAMPLE_DIVISOR: usize = 4;

/// Sentinel return address pushed onto the stack so JSR'd routines RTS to $0000,
/// where we plant an RTS opcode to halt single-stepping cleanly.
const STACK_HIGH: u16 = 0x01FF;
const STACK_LOW: u16 = 0x01FE;
const STACK_BASE: u16 = 0x0100;
const STACK_SENTINEL: u8 = 0xFF;
const STACK_POINTER_INIT: u8 = 0xFD;
const RTS_OPCODE: u8 = 0x60;
/// Where the CPU's PC parks when no init/play routine is running.
/// Init returns here (we exit `run_init`); PSID play routines RTS here too.
const SENTINEL_PC: u16 = 0x0000;
/// Cycle limit for a single play-routine call. Replaces the prior 100k step limit
/// (~3 cycles per instruction average → 300k cycles is the cycle equivalent).
const MAX_PLAY_CYCLES: u64 = 300_000;
/// Nominal 6502 IRQ entry cost (3 stack pushes + 2 vector reads).
const IRQ_ENTRY_CYCLES: u64 = 7;

/// How the play routine gets driven for the current tune.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DriveMode {
    /// PSID with `speed` bit clear: kick `play()` every PAL/NTSC frame.
    Frame,
    /// PSID with `speed` bit set: poll CIA1 Timer A; tune sets the latch.
    /// The "2x speed" tunes in HVSC pick this path.
    PsidCia,
    /// RSID (or PSID with `play_address == 0`): tune installs its own IRQ vector;
    /// mos6502 services CIA IRQ via the KERNAL stub.
    Interrupt,
}

/// One column of the piano roll: what a voice was playing at that instant.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NoteSample {
    /// MIDI note number (0 = no note).
    pub note: u8,
    /// Gate bit of the voice control register.
    pub gate: bool,
    /// Waveform bits (triangle/saw/pulse/noise) of the control register.
    pub waveform: u8,
}

/// A snapshot of one SID voice, used by the voice cards and the piano roll.
#[derive(Debug, Clone, Copy, Default)]
pub struct VoiceInfo {
    /// MIDI note derived from the frequency registers, if the gate is open.
    pub note: Option<u8>,
    /// Oscillator frequency in Hz.
    pub freq_hz: f32,
    pub gate: bool,
    /// Waveform bits: bit 0 triangle, 1 saw, 2 pulse, 3 noise.
    pub waveform: u8,
    /// Pulse width, 0.0 to 1.0.
    pub pulse_width: f32,
    pub ring: bool,
    pub sync: bool,
    pub test: bool,
}

impl VoiceInfo {
    /// Human-readable waveform name for the voice card.
    pub fn waveform_name(&self) -> String {
        if self.test {
            return "TEST".to_string();
        }
        let mut parts = Vec::new();
        if self.waveform & 0x1 != 0 {
            parts.push("TRI");
        }
        if self.waveform & 0x2 != 0 {
            parts.push("SAW");
        }
        if self.waveform & 0x4 != 0 {
            parts.push("PULSE");
        }
        if self.waveform & 0x8 != 0 {
            parts.push("NOISE");
        }
        if parts.is_empty() {
            "—".to_string()
        } else {
            parts.join("+")
        }
    }
}

/// Current SID filter configuration.
#[derive(Debug, Clone, Copy, Default)]
pub struct FilterInfo {
    /// 11-bit cutoff register value.
    pub cutoff: u16,
    /// Cutoff in Hz, approximated for the current clock.
    pub cutoff_hz: f32,
    /// Resonance, 0-15.
    pub resonance: u8,
    pub lowpass: bool,
    pub bandpass: bool,
    pub highpass: bool,
    /// Which voices are routed through the filter.
    pub routed: [bool; 3],
    /// Master volume, 0-15.
    pub volume: u8,
}


/// SID music player combining 6502 CPU and SID chip emulation.
///
/// Executes the SID tune's play routine at the correct frame rate while
/// generating audio samples. Supports PAL/NTSC timing, both SID chip models,
/// and multi-SID tunes (2-3 SIDs for 6-9 voices).
pub struct Player {
    /// 6502 CPU with C64 memory map
    cpu: CPU<C64Memory, Nmos6502>,
    /// Address of the play routine called each frame
    play_address: u16,
    /// Address of the init routine for song setup
    init_address: u16,
    /// Memory address where tune data is loaded
    load_address: u16,
    /// Original tune data for reloading on song change
    sid_data: Vec<u8>,
    /// CPU cycles per video frame (PAL: 19656, NTSC: 17045)
    cycles_per_frame: u32,
    /// Fractional cycles to run per audio sample
    cycles_per_sample: f64,
    /// Accumulated fractional cycles between samples
    cycle_accumulator: f64,
    /// Cycles elapsed in current frame
    frame_cycle_count: u32,
    /// Playback paused state
    paused: bool,
    /// Per-voice envelope history for oscilloscope display (3 per SID)
    envelope_history: Vec<Box<[f32; SCOPE_BUFFER_SIZE]>>,
    /// Write position in envelope ring buffers
    envelope_write_pos: usize,
    /// Counter for downsampling envelope captures
    envelope_sample_counter: usize,
    /// Per-voice ADSR stage history, captured with the envelope (0=A, 1=D/S, 2=R)
    envelope_state_history: Vec<Box<[u8; SCOPE_BUFFER_SIZE]>>,
    /// Newest mixed output samples, for the spectrum and the oscilloscope
    analysis_buffer: Box<[f32; ANALYSIS_BUFFER_SIZE]>,
    /// Write position in the analysis ring buffer
    analysis_write_pos: usize,
    /// Per-voice piano-roll history (3 per SID)
    note_history: Vec<Box<[NoteSample; NOTE_HISTORY_SIZE]>>,
    /// Write position in the note history ring buffers
    note_write_pos: usize,
    /// Counter for downsampling note captures
    note_sample_counter: usize,
    /// Samples between piano-roll columns (~100 columns per second)
    note_sample_divisor: usize,
    /// Chip models for each SID (1-3 entries)
    chip_models: Vec<ChipModel>,
    /// System clock frequency (PAL or NTSC)
    clock_hz: u32,
    /// Audio output sample rate
    sample_rate: u32,
    /// Last playback error (auto-pauses on error)
    playback_error: Option<String>,
    /// Resampling method for SID audio output
    sampling_method: SamplingMethod,
    /// Cycles since the CPU last left `SENTINEL_PC`; reset when it returns. Catches
    /// runaway play routines and IRQ handlers stuck in a tight loop.
    cpu_busy_cycles: u64,
    /// Recomputed from `speed_flags` and the current song each time the song or
    /// file changes — speed bits are per-song so a "1x" track followed by a "2x"
    /// track in the same file must re-derive.
    drive_mode: DriveMode,
    /// Set once at file-load. Skips the speed-flag check for RSID-style tunes.
    is_interrupt_driven: bool,
    /// Cached `SidFile::speed` so `load_song` can re-derive the drive mode without
    /// holding a `SidFile` reference.
    speed_flags: u32,
    /// `$01` banking byte to apply before running the init routine. Mirrors
    /// libsidplayfp's psiddrv `iomap`: tunes loaded into KERNAL or BASIC ROM
    /// space need those ROMs banked out so the CPU executes the loaded code.
    init_iomap: u8,
    /// `$01` banking byte to apply before each play call. Re-applied on every
    /// kick so the play routine sees the right banking even if init wrote $01.
    play_iomap: u8,
    /// User preference for the EKV transistor-model filter. Persisted on the
    /// player so it survives `load_sid_file`, which rebuilds the SID chips.
    ekv_filter: bool,
    /// True when the init routine did not return (RSID/KERNAL-emulation tunes
    /// whose init is also their main loop). The CPU then never parks at the
    /// sentinel, so the runaway-routine watchdog must stay out of the way.
    free_running: bool,
    /// Previous NMI line state for edge-triggered NMI detection.
    last_nmi_state: bool,
}

/// Errors that can occur while initializing or running SID routines.
#[derive(Debug, PartialEq, Eq)]
pub enum PlayerError {
    /// The init routine never returned before the step limit.
    InitTimeout { steps: u32, address: u16 },
}

impl fmt::Display for PlayerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InitTimeout { steps, address } => {
                write!(
                    f,
                    "SID init routine at ${address:04X} exceeded {steps} steps"
                )
            }
        }
    }
}

impl error::Error for PlayerError {}

type PlayerResult<T> = Result<T, PlayerError>;

impl Player {
    /// Creates a player for the given SID file and song number (1-indexed).
    ///
    /// Loads the tune into emulated memory, runs the init routine, and
    /// configures timing based on PAL/NTSC detection from the file header.
    ///
    /// The `sampling_method` parameter controls audio quality vs CPU usage:
    /// - `Fast`: Direct output (lowest quality, lowest CPU)
    /// - `Interpolate`: Linear interpolation (good quality, low CPU)
    /// - `ResampleFast`: FIR resampling without interpolation
    /// - `Resample`: FIR resampling with interpolation (highest quality)
    /// - `ResampleTwoPass`: Two-stage FIR resampling (high quality, efficient)
    pub fn new(
        sid_file: &SidFile,
        song: u16,
        sample_rate: u32,
        chip_override: Option<ChipModel>,
        sampling_method: SamplingMethod,
    ) -> PlayerResult<Self> {
        let (clock_hz, cycles_per_frame) = timing_from_file(sid_file);
        let chip_models = sid_file.preferred_chip_models(chip_override);

        let mut cpu = bootstrap_cpu(
            sid_file,
            &chip_models,
            sample_rate,
            clock_hz,
            song,
            sampling_method,
        );

        // Apply PSID-driver banking before init so tunes whose code lives in
        // BASIC or KERNAL ROM space (e.g. Cobra at $F100) execute from RAM.
        cpu.memory.set_bank_config(psid_iomap(sid_file.init_address));
        let init_returned = run_init(
            &mut cpu,
            sid_file.init_address,
            sid_file.is_interrupt_driven(),
        )?;

        let voice_count = chip_models.len() * 3;
        let envelope_history = (0..voice_count)
            .map(|_| Box::new([0.0; SCOPE_BUFFER_SIZE]))
            .collect();
        let envelope_state_history = (0..voice_count)
            .map(|_| Box::new([0u8; SCOPE_BUFFER_SIZE]))
            .collect();
        let note_history = (0..voice_count)
            .map(|_| Box::new([NoteSample::default(); NOTE_HISTORY_SIZE]))
            .collect();
        let note_sample_divisor = (sample_rate / 100).max(1) as usize;

        let mut player = Self {
            cpu,
            play_address: sid_file.play_address,
            init_address: sid_file.init_address,
            load_address: sid_file.load_address,
            sid_data: sid_file.data.clone(),
            cycles_per_frame,
            cycles_per_sample: f64::from(clock_hz) / f64::from(sample_rate),
            cycle_accumulator: 0.0,
            frame_cycle_count: 0,
            paused: false,
            envelope_history,
            envelope_write_pos: 0,
            envelope_sample_counter: 0,
            envelope_state_history,
            analysis_buffer: Box::new([0.0; ANALYSIS_BUFFER_SIZE]),
            analysis_write_pos: 0,
            note_history,
            note_write_pos: 0,
            note_sample_counter: 0,
            note_sample_divisor,
            chip_models,
            clock_hz,
            sample_rate,
            playback_error: None,
            sampling_method,
            cpu_busy_cycles: 0,
            drive_mode: DriveMode::Frame,
            is_interrupt_driven: sid_file.is_interrupt_driven(),
            speed_flags: sid_file.speed,
            init_iomap: psid_iomap(sid_file.init_address),
            play_iomap: psid_iomap(sid_file.play_address),
            ekv_filter: false,
            free_running: !init_returned,
            last_nmi_state: false,
        };
        player.apply_drive_mode(song);
        Ok(player)
    }

    /// Fills the buffer with audio samples, advancing emulation accordingly.
    ///
    /// Cycle-accurate: every CPU instruction is single-stepped, and CIA/SID/audio
    /// timers all advance by the same `elapsed` cycle delta from `cpu.cycles`.
    /// On error, auto-pauses and stores error message for TUI to display.
    pub fn fill_buffer(&mut self, buffer: &mut [f32]) {
        if self.paused || self.playback_error.is_some() {
            buffer.fill(0.0);
            return;
        }

        let sid_count = self.cpu.memory.sids.len();

        for sample in buffer.iter_mut() {
            self.cycle_accumulator += self.cycles_per_sample;
            while self.cycle_accumulator >= 1.0 {
                if !self.advance_one_step() {
                    buffer.fill(0.0);
                    return;
                }
            }

            let sum: i32 = self
                .cpu
                .memory
                .sids
                .iter()
                .map(|s| i32::from(s.sid.output()))
                .sum();
            *sample = mix_sample(sum, sid_count);

            self.analysis_buffer[self.analysis_write_pos] = *sample;
            self.analysis_write_pos = (self.analysis_write_pos + 1) % ANALYSIS_BUFFER_SIZE;

            self.note_sample_counter += 1;
            if self.note_sample_counter >= self.note_sample_divisor {
                self.note_sample_counter = 0;
                self.capture_notes();
            }

            self.capture_envelope_history();
        }
    }

    /// Fast-forwards playback by `seconds` of tune time without rendering audio.
    ///
    /// SID players have no random access, so seeking rebuilds the emulator state
    /// by running the tune forward. When the CPU idles at the sentinel, the rest
    /// of the frame is advanced in one step instead of one cycle at a time, and
    /// no audio samples are produced, which keeps a two-minute jump responsive.
    /// `cancel` is polled between batches so a newer seek can abandon this one.
    pub fn fast_forward(&mut self, seconds: f32, cancel: &AtomicBool) {
        let target = (f64::from(seconds.max(0.0)) * f64::from(self.clock_hz)) as u64;
        let mut done: u64 = 0;

        while done < target {
            if cancel.load(Ordering::Relaxed) {
                break;
            }

            let idle = self.drive_mode != DriveMode::Interrupt
                && self.cpu.registers.program_counter == SENTINEL_PC;

            if idle {
                let chunk = if self.drive_mode == DriveMode::Frame {
                    self.cycles_per_frame
                        .saturating_sub(self.frame_cycle_count)
                        .max(1)
                } else {
                    // PSID/CIA tunes kick play on timer underflow; small chunks
                    // keep the sequencer's own timing intact while seeking.
                    self.cycles_per_frame.min(1024)
                };
                let step = u64::from(chunk).min(target - done);
                #[allow(clippy::cast_possible_truncation)]
                let step_u32 = step as u32;

                if self.drive_mode != DriveMode::Frame {
                    self.cpu.memory.tick(step);
                }
                for sid_chip in &mut self.cpu.memory.sids {
                    sid_chip.sid.clock_delta(step_u32);
                }
                self.cpu.cycles += step;
                self.frame_cycle_count = self.frame_cycle_count.saturating_add(step_u32);
                done += step;

                if self.should_kick_play() {
                    reset_stack_pointer(&mut self.cpu);
                    self.cpu.memory.set_bank_config(self.play_iomap);
                    self.cpu.registers.program_counter = self.play_address;
                }
                continue;
            }

            let before = self.cpu.cycles;
            if !self.advance_one_step() {
                break;
            }
            done += self.cpu.cycles - before;
        }

        // `advance_one_step` keeps subtracting from the sample accumulator; start
        // the next audio callback from a clean phase.
        self.cycle_accumulator = 0.0;
        self.cpu_busy_cycles = 0;
    }

    /// Drives the emulator forward by one CPU instruction (or one idle cycle when no
    /// routine is running) and ticks all per-cycle peripherals by the same delta.
    /// Returns `false` to halt the buffer (runaway routine, etc.).
    fn advance_one_step(&mut self) -> bool {
        let at_sentinel = self.cpu.registers.program_counter == SENTINEL_PC;

        // NMI is edge-triggered (rising edge on CIA2 IRQ line) and non-maskable.
        let nmi_current =
            self.drive_mode == DriveMode::Interrupt && self.cpu.memory.raw_nmi_pending();
        let service_nmi = nmi_current && !self.last_nmi_state;
        self.last_nmi_state = nmi_current;

        // Only RSID-style tunes route CIA/VIC IRQs through the tune's vector. PSID
        // (frame or CIA-poll) keeps the CPU at the sentinel and the player calls
        // play() directly. `at_sentinel` is NOT required here: free-running RSIDs
        // loop in user RAM while still taking interrupts.
        //
        // `mos6502` also polls the bus for interrupts after each instruction, so
        // `Bus::irq_pending`/`nmi_pending` deliberately return false. Otherwise its
        // independent edge latch would service the same NMI again with a stale
        // return address, running the tune's handler twice per timer underflow.
        let service_irq = !service_nmi
            && self.drive_mode == DriveMode::Interrupt
            && self.cpu.memory.raw_irq_pending()
            && !self
                .cpu
                .registers
                .status
                .contains(Status::PS_DISABLE_INTERRUPTS);

        let prev_cycles = self.cpu.cycles;
        let mut elapsed = if service_nmi {
            self.force_nmi_service();
            IRQ_ENTRY_CYCLES
        } else if service_irq {
            // Manually vector: at the sentinel an RTS would pop the planted dummy
            // return address instead of a real interrupted PC.
            self.force_irq_service();
            IRQ_ENTRY_CYCLES
        } else if at_sentinel {
            1
        } else {
            // A malformed tune can drive mos6502 into an unimplemented opcode,
            // which panics. Letting that panic escape the audio callback would
            // poison the player mutex and make every UI button silently stop
            // working, so turn it into a normal playback error instead.
            let stepped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                self.cpu.single_step();
            }));
            if stepped.is_err() {
                self.playback_error = Some(format!(
                    "Invalid CPU opcode at ${:04X}",
                    self.cpu.registers.program_counter
                ));
                self.paused = true;
                return false;
            }
            self.cpu.cycles - prev_cycles
        };
        if elapsed == 0 {
            // WAI/STP (or an undecodable opcode) completes without consuming a
            // cycle in mos6502. Charge one idle cycle anyway so the audio loop
            // makes progress and the VIC/CIA can eventually raise the interrupt
            // that wakes the CPU instead of spinning here forever.
            elapsed = 1;
        }
        // Forced interrupt entry and sentinel idle do not move `cpu.cycles`, but
        // the peripherals and sample clock are charged for them; keep the CPU's
        // own cycle counter in step with that accounting.
        self.cpu.cycles = prev_cycles.saturating_add(elapsed);

        // Frame-mode tunes never observe CIA1, so skip the timer arithmetic entirely.
        if self.drive_mode != DriveMode::Frame {
            self.cpu.memory.tick(elapsed);
        }
        // If the handler acked CIA2 during this step, the NMI line was low for a
        // moment. Re-arm the edge latch so a timer underflow that happens later
        // in this same tick is still seen as a fresh rising edge; sampling only
        // at instruction boundaries would miss it and stall NMI-driven tunes.
        if self.cpu.memory.take_nmi_ack_seen() {
            self.last_nmi_state = false;
        }
        #[allow(clippy::cast_possible_truncation)]
        let elapsed_u32 = elapsed as u32;
        for sid_chip in &mut self.cpu.memory.sids {
            sid_chip.sid.clock_delta(elapsed_u32);
        }

        self.cycle_accumulator -= elapsed as f64;
        self.frame_cycle_count = self.frame_cycle_count.saturating_add(elapsed_u32);

        // PC away from the sentinel = a play routine or IRQ handler is running;
        // a too-long stretch trips the runaway-routine timeout. Interrupt-driven
        // RSID tunes (and free-running ones) legitimately keep executing a main
        // loop between IRQs and never return to the sentinel, so the cumulative
        // guard does not apply to them.
        if self.cpu.registers.program_counter != SENTINEL_PC {
            if !self.free_running && self.drive_mode != DriveMode::Interrupt {
                self.cpu_busy_cycles = self.cpu_busy_cycles.saturating_add(elapsed);
                if self.cpu_busy_cycles > MAX_PLAY_CYCLES {
                    self.playback_error = Some(format!(
                        "CPU stuck for {MAX_PLAY_CYCLES} cycles at PC=${:04X}",
                        self.cpu.registers.program_counter
                    ));
                    self.paused = true;
                    return false;
                }
            }
            return true;
        }
        self.cpu_busy_cycles = 0;

        if self.should_kick_play() {
            reset_stack_pointer(&mut self.cpu);
            // Re-apply banking on every kick: init may have left $01 elsewhere,
            // and libsidplayfp's psiddrv refreshes it before each play call.
            self.cpu.memory.set_bank_config(self.play_iomap);
            self.cpu.registers.program_counter = self.play_address;
        }

        true
    }

    /// True when the CPU is idle at the sentinel and the active drive-mode says
    /// it's time to invoke `play()`. RSID/Interrupt mode never kicks here — it
    /// services the IRQ via the CPU vector path instead.
    fn should_kick_play(&mut self) -> bool {
        if self.play_address == 0 {
            return false;
        }
        match self.drive_mode {
            DriveMode::Frame => {
                if self.frame_cycle_count >= self.cycles_per_frame {
                    self.frame_cycle_count -= self.cycles_per_frame;
                    true
                } else {
                    false
                }
            }
            DriveMode::PsidCia => self.cpu.memory.cia1_take_timer_a_underflow(),
            DriveMode::Interrupt => false,
        }
    }

    /// Mimics mos6502's `service_interrupt` without executing whatever opcode lives
    /// at the current PC. Used to wake the CPU from the sentinel when CIA1/VIC fires.
    fn force_irq_service(&mut self) {
        let [pc_lo, pc_hi] = self.cpu.registers.program_counter.to_le_bytes();
        self.push_byte(pc_hi);
        self.push_byte(pc_lo);
        let status = self.cpu.registers.status & !Status::PS_BRK;
        self.push_byte(status.bits());
        self.cpu
            .registers
            .status
            .insert(Status::PS_DISABLE_INTERRUPTS);
        let lo = self.cpu.memory.get_byte(IRQ_INTERRUPT_VECTOR_LO);
        let hi = self.cpu.memory.get_byte(IRQ_INTERRUPT_VECTOR_HI);
        self.cpu.registers.program_counter = u16::from_le_bytes([lo, hi]);
    }

    /// Services a rising-edge NMI (CIA2) through `$FFFA/$FFFB`.
    fn force_nmi_service(&mut self) {
        let [pc_lo, pc_hi] = self.cpu.registers.program_counter.to_le_bytes();
        self.push_byte(pc_hi);
        self.push_byte(pc_lo);
        let status = self.cpu.registers.status & !Status::PS_BRK;
        self.push_byte(status.bits());
        self.cpu
            .registers
            .status
            .insert(Status::PS_DISABLE_INTERRUPTS);
        let lo = self.cpu.memory.get_byte(NMI_INTERRUPT_VECTOR_LO);
        let hi = self.cpu.memory.get_byte(NMI_INTERRUPT_VECTOR_HI);
        self.cpu.registers.program_counter = u16::from_le_bytes([lo, hi]);
    }

    fn push_byte(&mut self, val: u8) {
        let sp = self.cpu.registers.stack_pointer.0;
        self.cpu.memory.set_byte(STACK_BASE | u16::from(sp), val);
        self.cpu.registers.stack_pointer = StackPointer(sp.wrapping_sub(1));
    }

    /// Captures envelope history at reduced rate for oscilloscope display.
    fn capture_envelope_history(&mut self) {
        self.envelope_sample_counter += 1;
        if self.envelope_sample_counter < ENVELOPE_SAMPLE_DIVISOR {
            return;
        }
        self.envelope_sample_counter = 0;

        let mut voice_idx = 0;
        for sid_chip in &self.cpu.memory.sids {
            let state = sid_chip.sid.read_state();
            for (v, &env) in state.envelope_counter.iter().enumerate() {
                if voice_idx < self.envelope_history.len() {
                    self.envelope_history[voice_idx][self.envelope_write_pos] =
                        f32::from(env) / 255.0;
                    self.envelope_state_history[voice_idx][self.envelope_write_pos] =
                        state.envelope_state[v];
                }
                voice_idx += 1;
            }
        }
        self.envelope_write_pos = (self.envelope_write_pos + 1) % SCOPE_BUFFER_SIZE;
    }

    /// Captures the note and gate state of every voice for the piano roll.
    fn capture_notes(&mut self) {
        let mut voice_idx = 0;
        for sid_chip in &self.cpu.memory.sids {
            let state = sid_chip.sid.read_state();
            for voice in 0..3 {
                if voice_idx >= self.note_history.len() {
                    break;
                }
                let reg = voice * 7;
                let freq = u16::from(state.sid_register[reg])
                    | (u16::from(state.sid_register[reg + 1]) << 8);
                let control = state.sid_register[reg + 4];
                let gate = control & 0x01 != 0;
                let note = if gate && freq > 0 {
                    freq_to_note(freq, self.clock_hz)
                } else {
                    0
                };
                self.note_history[voice_idx][self.note_write_pos] = NoteSample {
                    note,
                    gate,
                    waveform: (control >> 4) & 0x0F,
                };
                voice_idx += 1;
            }
        }
        self.note_write_pos = (self.note_write_pos + 1) % NOTE_HISTORY_SIZE;
    }

    /// Copies envelope history for each voice into `out`, reusing existing buffers.
    pub fn copy_envelope_samples_into(&self, out: &mut Vec<Vec<f32>>) {
        let voice_count = self.envelope_history.len();
        out.resize_with(voice_count, Vec::new);
        if self.paused {
            for buf in out.iter_mut() {
                buf.resize(SCOPE_BUFFER_SIZE, 0.0);
                buf.fill(0.0);
            }
            return;
        }
        for (buf, history) in out.iter_mut().zip(self.envelope_history.iter()) {
            let (head, tail) = history.split_at(self.envelope_write_pos);
            buf.clear();
            buf.extend_from_slice(tail);
            buf.extend_from_slice(head);
        }
    }

    /// Copies newest mixed output samples into `out`, reusing its allocation.
    pub fn copy_audio_samples_into(&self, out: &mut Vec<f32>) {
        if self.paused {
            out.resize(ANALYSIS_BUFFER_SIZE, 0.0);
            out.fill(0.0);
            return;
        }
        let (head, tail) = self.analysis_buffer.split_at(self.analysis_write_pos);
        out.clear();
        out.extend_from_slice(tail);
        out.extend_from_slice(head);
    }

    /// Copies ADSR stage history per voice into `out`, reusing existing buffers.
    pub fn copy_envelope_states_into(&self, out: &mut Vec<Vec<u8>>) {
        out.resize_with(self.envelope_state_history.len(), Vec::new);
        for (buf, history) in out.iter_mut().zip(self.envelope_state_history.iter()) {
            let (head, tail) = history.split_at(self.envelope_write_pos);
            buf.clear();
            buf.extend_from_slice(tail);
            buf.extend_from_slice(head);
        }
    }

    /// Copies piano-roll history per voice into `out`, reusing existing buffers.
    pub fn copy_note_history_into(&self, out: &mut Vec<Vec<NoteSample>>) {
        out.resize_with(self.note_history.len(), Vec::new);
        for (buf, history) in out.iter_mut().zip(self.note_history.iter()) {
            let (head, tail) = history.split_at(self.note_write_pos);
            buf.clear();
            buf.extend_from_slice(tail);
            buf.extend_from_slice(head);
        }
    }

    /// Copies current register state of every voice into `out`, reusing its allocation.
    pub fn copy_voice_infos_into(&self, out: &mut Vec<VoiceInfo>) {
        out.clear();
        for sid_chip in &self.cpu.memory.sids {
            let state = sid_chip.sid.read_state();
            for voice in 0..3 {
                let reg = voice * 7;
                let freq = u16::from(state.sid_register[reg])
                    | (u16::from(state.sid_register[reg + 1]) << 8);
                let pulse_width = u16::from(state.sid_register[reg + 2])
                    | ((u16::from(state.sid_register[reg + 3]) & 0x0F) << 8);
                let control = state.sid_register[reg + 4];
                let gate = control & 0x01 != 0;
                let freq_hz = f32::from(freq) * self.clock_hz as f32 / 16_777_216.0;
                out.push(VoiceInfo {
                    note: (gate && freq > 0).then(|| freq_to_note(freq, self.clock_hz)),
                    freq_hz,
                    gate,
                    waveform: (control >> 4) & 0x0F,
                    pulse_width: f32::from(pulse_width) / 4095.0,
                    ring: control & 0x04 != 0,
                    sync: control & 0x02 != 0,
                    test: control & 0x08 != 0,
                });
            }
        }
    }

    /// Copies current filter configuration of every SID chip into `out`, reusing its allocation.
    pub fn copy_filter_infos_into(&self, out: &mut Vec<FilterInfo>) {
        out.clear();
        for sid_chip in &self.cpu.memory.sids {
            let state = sid_chip.sid.read_state();
            let reg = &state.sid_register;
            let cutoff = u16::from(reg[0x15]) | (u16::from(reg[0x16] & 0x07) << 8);
            let res_routing = reg[0x17];
            let mode_volume = reg[0x18];
            out.push(FilterInfo {
                cutoff,
                cutoff_hz: 30.0
                    + f32::from(cutoff) * 5.8 * (self.clock_hz as f32 / 1_000_000.0),
                resonance: res_routing >> 4,
                lowpass: mode_volume & 0x10 != 0,
                bandpass: mode_volume & 0x20 != 0,
                highpass: mode_volume & 0x40 != 0,
                routed: [
                    res_routing & 0x01 != 0,
                    res_routing & 0x02 != 0,
                    res_routing & 0x04 != 0,
                ],
                volume: mode_volume & 0x0F,
            });
        }
    }

    /// Copies envelope levels (0-255) for all SID voices into `out`, reusing its allocation.
    pub fn copy_voice_levels_into(&self, out: &mut Vec<u8>) {
        let voice_count = self.cpu.memory.sids.len() * 3;
        if self.paused {
            out.resize(voice_count, 0);
            out.fill(0);
            return;
        }
        out.clear();
        for s in &self.cpu.memory.sids {
            out.extend_from_slice(&s.sid.read_state().envelope_counter);
        }
    }

    /// Copies per-voice mute switches into `out`, reusing its allocation.
    pub fn copy_voice_mutes_into(&self, out: &mut Vec<bool>) {
        out.clear();
        for chip in &self.cpu.memory.sids {
            for voice in 0..3 {
                out.push(chip.voice_muted(voice));
            }
        }
    }

    /// Returns envelope history for each voice, ordered oldest to newest.
    /// Returns 3 entries per SID (3/6/9 voices for 1/2/3 SIDs).
    #[allow(dead_code)]
    pub fn envelope_samples(&self) -> Vec<Vec<f32>> {
        let voice_count = self.envelope_history.len();
        if self.paused {
            return vec![vec![0.0; SCOPE_BUFFER_SIZE]; voice_count];
        }
        self.envelope_history
            .iter()
            .map(|history| {
                // Older samples sit after the write head; chain them in front of the newer prefix.
                let (head, tail) = history.split_at(self.envelope_write_pos);
                tail.iter().chain(head).copied().collect()
            })
            .collect()
    }

    /// Newest mixed output samples, ordered oldest to newest.
    ///
    /// Used by the spectrum analyser and the triggered oscilloscope.
    #[allow(dead_code)]
    pub fn audio_samples(&self) -> Vec<f32> {
        if self.paused {
            return vec![0.0; ANALYSIS_BUFFER_SIZE];
        }
        let (head, tail) = self.analysis_buffer.split_at(self.analysis_write_pos);
        tail.iter().chain(head).copied().collect()
    }

    /// ADSR stage history per voice, ordered oldest to newest.
    /// `0` = attack, `1` = decay/sustain, `2` = release.
    #[allow(dead_code)]
    pub fn envelope_states(&self) -> Vec<Vec<u8>> {
        self.envelope_state_history
            .iter()
            .map(|history| {
                let (head, tail) = history.split_at(self.envelope_write_pos);
                tail.iter().chain(head).copied().collect()
            })
            .collect()
    }

    /// Piano-roll history per voice, ordered oldest to newest.
    #[allow(dead_code)]
    pub fn note_history(&self) -> Vec<Vec<NoteSample>> {
        self.note_history
            .iter()
            .map(|history| {
                let (head, tail) = history.split_at(self.note_write_pos);
                tail.iter().chain(head).copied().collect()
            })
            .collect()
    }

    /// Current register state of every voice (3 per SID).
    #[allow(dead_code)]
    pub fn voice_infos(&self) -> Vec<VoiceInfo> {
        let mut infos = Vec::with_capacity(self.cpu.memory.sids.len() * 3);
        for sid_chip in &self.cpu.memory.sids {
            let state = sid_chip.sid.read_state();
            for voice in 0..3 {
                let reg = voice * 7;
                let freq = u16::from(state.sid_register[reg])
                    | (u16::from(state.sid_register[reg + 1]) << 8);
                let pulse_width = u16::from(state.sid_register[reg + 2])
                    | ((u16::from(state.sid_register[reg + 3]) & 0x0F) << 8);
                let control = state.sid_register[reg + 4];
                let gate = control & 0x01 != 0;
                let freq_hz = f32::from(freq) * self.clock_hz as f32 / 16_777_216.0;
                infos.push(VoiceInfo {
                    note: (gate && freq > 0).then(|| freq_to_note(freq, self.clock_hz)),
                    freq_hz,
                    gate,
                    waveform: (control >> 4) & 0x0F,
                    pulse_width: f32::from(pulse_width) / 4095.0,
                    ring: control & 0x04 != 0,
                    sync: control & 0x02 != 0,
                    test: control & 0x08 != 0,
                });
            }
        }
        infos
    }

    /// Current filter configuration of every SID chip.
    #[allow(dead_code)]
    pub fn filter_infos(&self) -> Vec<FilterInfo> {
        self.cpu
            .memory
            .sids
            .iter()
            .map(|sid_chip| {
                let state = sid_chip.sid.read_state();
                let reg = &state.sid_register;
                let cutoff = u16::from(reg[0x15]) | (u16::from(reg[0x16] & 0x07) << 8);
                let res_routing = reg[0x17];
                let mode_volume = reg[0x18];
                FilterInfo {
                    cutoff,
                    cutoff_hz: 30.0
                        + f32::from(cutoff) * 5.8 * (self.clock_hz as f32 / 1_000_000.0),
                    resonance: res_routing >> 4,
                    lowpass: mode_volume & 0x10 != 0,
                    bandpass: mode_volume & 0x20 != 0,
                    highpass: mode_volume & 0x40 != 0,
                    routed: [
                        res_routing & 0x01 != 0,
                        res_routing & 0x02 != 0,
                        res_routing & 0x04 != 0,
                    ],
                    volume: mode_volume & 0x0F,
                }
            })
            .collect()
    }

    /// Toggles between playing and paused states.
    pub const fn toggle_pause(&mut self) {
        self.paused = !self.paused;
    }

    /// Returns whether playback is currently paused.
    pub const fn is_paused(&self) -> bool {
        self.paused
    }

    /// Takes and clears any pending playback error.
    #[allow(dead_code)] // surfaced once the UI has an error banner
    pub fn take_error(&mut self) -> Option<String> {
        self.playback_error.take()
    }

    /// Loads a completely new SID file, replacing the current tune.
    pub fn load_sid_file(&mut self, sid_file: &SidFile, song: u16) -> PlayerResult<()> {
        (self.clock_hz, self.cycles_per_frame) = timing_from_file(sid_file);
        self.cycles_per_sample = f64::from(self.clock_hz) / f64::from(self.sample_rate);

        self.play_address = sid_file.play_address;
        self.init_address = sid_file.init_address;
        self.load_address = sid_file.load_address;
        self.sid_data = sid_file.data.clone();
        self.is_interrupt_driven = sid_file.is_interrupt_driven();
        self.speed_flags = sid_file.speed;
        self.init_iomap = psid_iomap(sid_file.init_address);
        self.play_iomap = psid_iomap(sid_file.play_address);

        self.chip_models = sid_file.preferred_chip_models(None);
        let sid_configs = build_sid_configs(sid_file, &self.chip_models);
        self.cpu.memory.configure_sids(&sid_configs);
        self.cpu.memory.set_pal(sid_file.is_pal());

        for sid_chip in &mut self.cpu.memory.sids {
            sid_chip
                .sid
                .set_sampling_parameters(self.sampling_method, self.clock_hz, self.sample_rate)
                .unwrap();
        }
        // configure_sids rebuilds chips with the default (standard) filter — re-apply preference.
        self.sync_ekv_filter();

        let voice_count = self.chip_models.len() * 3;
        self.envelope_history = (0..voice_count)
            .map(|_| Box::new([0.0; SCOPE_BUFFER_SIZE]))
            .collect();
        self.envelope_state_history = (0..voice_count)
            .map(|_| Box::new([0u8; SCOPE_BUFFER_SIZE]))
            .collect();
        self.note_history = (0..voice_count)
            .map(|_| Box::new([NoteSample::default(); NOTE_HISTORY_SIZE]))
            .collect();
        self.envelope_write_pos = 0;
        self.envelope_sample_counter = 0;
        self.analysis_buffer.fill(0.0);
        self.analysis_write_pos = 0;
        self.note_write_pos = 0;
        self.note_sample_counter = 0;

        self.load_song(song)?;
        Ok(())
    }

    /// Reinitialize for a different song number (1-indexed).
    /// Reloads SID data, resets CPU state, and runs the init routine.
    pub fn load_song(&mut self, song: u16) -> PlayerResult<()> {
        self.cpu.memory.clear_ram();
        self.cpu.memory.load(self.load_address, &self.sid_data);

        for sid_chip in &mut self.cpu.memory.sids {
            sid_chip.sid.reset();
            sid_chip.forget_voice_controls();
        }

        self.cpu.registers.index_x = 0;
        self.cpu.registers.index_y = 0;
        self.cpu.registers.status = mos6502::registers::Status::empty();

        setup_stack_for_rts(&mut self.cpu);
        #[allow(clippy::cast_possible_truncation)]
        let song_index = song.saturating_sub(1) as u8;
        self.cpu.registers.accumulator = song_index;
        self.cpu.registers.program_counter = self.init_address;

        // Apply PSID-driver banking before init so tunes loaded into KERNAL or
        // BASIC ROM space (e.g. Cobra at $F100) execute from RAM, not the stub.
        self.cpu.memory.set_bank_config(self.init_iomap);
        let init_returned =
            run_init(&mut self.cpu, self.init_address, self.is_interrupt_driven)?;
        self.free_running = !init_returned;

        // Drive mode is per-song (PSID `speed` is a 32-bit-per-song bitmap), so
        // re-derive even for an in-file song change.
        self.apply_drive_mode(song);

        self.cycle_accumulator = 0.0;
        self.frame_cycle_count = 0;
        self.cpu_busy_cycles = 0;
        self.last_nmi_state = false;
        self.paused = false;
        self.playback_error = None;
        Ok(())
    }

    /// Picks the play-dispatch mode for `song` and pushes the bus IRQ-routing flag
    /// in lockstep so PSID modes never see mos6502 service a CIA IRQ.
    fn apply_drive_mode(&mut self, song: u16) {
        self.drive_mode = if self.is_interrupt_driven {
            DriveMode::Interrupt
        } else if cia_timing_for_song(self.speed_flags, song) {
            DriveMode::PsidCia
        } else {
            DriveMode::Frame
        };
        self.cpu
            .memory
            .set_cia_irq_routed(self.drive_mode == DriveMode::Interrupt);
    }

    /// Returns envelope levels (0-255) for all SID voices.
    /// Returns 3 entries per SID (3/6/9 voices for 1/2/3 SIDs).
    /// Unlike hardware where only ENV3 ($D41C) is readable, emulation
    /// gives us direct access to all voice envelopes via internal state.
    #[allow(dead_code)]
    pub fn voice_levels(&self) -> Vec<u8> {
        let voice_count = self.cpu.memory.sids.len() * 3;
        if self.paused {
            return vec![0; voice_count];
        }
        self.cpu
            .memory
            .sids
            .iter()
            .flat_map(|s| s.sid.read_state().envelope_counter)
            .collect()
    }

    /// Returns the per-voice mute switches, ordered chip by chip (3 per SID).
    #[allow(dead_code)]
    pub fn voice_mutes(&self) -> Vec<bool> {
        self.cpu.memory.voice_mutes()
    }

    /// Silences or restores one voice by its global index (chip * 3 + voice).
    pub fn set_voice_muted(&mut self, voice_index: usize, muted: bool) {
        self.cpu.memory.set_voice_muted(voice_index, muted);
    }

    /// Returns the chip models for all SIDs.
    pub fn chip_models(&self) -> &[ChipModel] {
        &self.chip_models
    }

    /// Returns the number of SID chips.
    pub fn sid_count(&self) -> usize {
        self.chip_models.len()
    }

    /// Cycles the chip model for the specified SID (0-indexed).
    /// Returns the new model, or `None` if `sid_index` is out of range.
    pub fn switch_chip_model(&mut self, sid_index: usize) -> Option<ChipModel> {
        if sid_index >= self.chip_models.len() {
            return None;
        }

        // Save current register state before replacing the chip.
        let state = self.cpu.memory.sids[sid_index].sid.read_state();

        let new_model = match self.chip_models[sid_index] {
            ChipModel::Mos6581 => ChipModel::Mos8580,
            ChipModel::Mos8580 => ChipModel::Mos6581,
        };
        self.chip_models[sid_index] = new_model;

        self.cpu.memory.set_chip_model(sid_index, new_model);
        self.cpu.memory.sids[sid_index]
            .sid
            .set_sampling_parameters(self.sampling_method, self.clock_hz, self.sample_rate)
            .unwrap();

        // Restore writable registers (0x00-0x18) to maintain playback.
        for (reg, &val) in state.sid_register[..0x19].iter().enumerate() {
            #[allow(clippy::cast_possible_truncation)]
            self.cpu.memory.sids[sid_index].sid.write(reg as u8, val);
        }

        Some(new_model)
    }

    /// Sets the EKV transistor-model filter preference for all SIDs.
    ///
    /// The preference is stored on the player and re-applied each time a new
    /// SID file is loaded (which rebuilds the chips and would otherwise reset
    /// the filter to the standard spline model). Only affects 6581 chips;
    /// 8580 always uses the standard filter.
    pub fn set_ekv_filter(&mut self, enable: bool) {
        self.ekv_filter = enable;
        self.sync_ekv_filter();
    }

    /// Forces every SID chip's EKV state to match `self.ekv_filter`.
    /// The underlying library only exposes a toggle, so we read the current
    /// state per chip and flip if it disagrees with the preference.
    fn sync_ekv_filter(&mut self) {
        for chip in &mut self.cpu.memory.sids {
            if chip.sid.is_ekv_filter_enabled() != self.ekv_filter {
                chip.sid.toggle_ekv_filter();
            }
        }
    }
}

/// PSID-driver `$01` value to apply for an entry-point address. Mirrors
/// libsidplayfp's `iomap`: tunes whose code lives in BASIC or KERNAL ROM space
/// need those ROMs banked out so the CPU executes the loaded RAM instead.
/// I/O at `$D000-$DFFF` stays mapped except when the tune itself sits there.
const fn psid_iomap(addr: u16) -> u8 {
    match addr {
        // $C000-$CFFF is plain RAM on the C64, so both ROMs can stay banked in.
        // Banking the KERNAL out here would replace its IRQ trampoline at
        // $FFFE/$FFFF with uninitialised RAM, breaking tunes that only install
        // their handler through the KERNAL's $0314 vector.
        0x0000..=0x9FFF => 0x37, // C64 default: BASIC + KERNAL + I/O all in
        0xA000..=0xBFFF => 0x36, // BASIC out (tune lives in BASIC ROM space)
        0xC000..=0xCFFF => 0x37, // plain RAM; KERNAL can stay banked in
        0xD000..=0xDFFF => 0x34, // BASIC + KERNAL + I/O out (tune in I/O area)
        0xE000..=0xFFFF => 0x35, // BASIC + KERNAL out, I/O in
    }
}

/// PSID `speed` field is a per-song bitmap (bit n = song n+1, 1 = CIA timer).
fn cia_timing_for_song(speed_flags: u32, song: u16) -> bool {
    if song == 0 || song > 32 {
        return false;
    }
    (speed_flags >> (song - 1)) & 1 != 0
}

fn timing_from_file(sid_file: &SidFile) -> (u32, u32) {
    if sid_file.is_pal() {
        (clock::PAL, PAL_FRAME_CYCLES)
    } else {
        (clock::NTSC, NTSC_FRAME_CYCLES)
    }
}

fn build_sid_configs(sid_file: &SidFile, chip_models: &[ChipModel]) -> Vec<(u16, ChipModel)> {
    sid_file
        .sid_addresses()
        .into_iter()
        .zip(chip_models.iter().copied())
        .collect()
}

fn bootstrap_cpu(
    sid_file: &SidFile,
    chip_models: &[ChipModel],
    sample_rate: u32,
    clock_hz: u32,
    song: u16,
    sampling_method: SamplingMethod,
) -> CPU<C64Memory, Nmos6502> {
    let mut memory = C64Memory::new(chip_models[0]);

    let sid_configs = build_sid_configs(sid_file, chip_models);
    memory.configure_sids(&sid_configs);

    for sid_chip in &mut memory.sids {
        sid_chip
            .sid
            .set_sampling_parameters(sampling_method, clock_hz, sample_rate)
            .unwrap();
    }

    memory.load(sid_file.load_address, &sid_file.data);
    memory.set_pal(sid_file.is_pal());

    let mut cpu = CPU::new(memory, Nmos6502);
    // mos6502 boots with the I flag set. On a real C64 the KERNAL clears it before
    // handing control to a tune; do the same here or RSID raster/CIA IRQs can
    // never be serviced on the first song. `load_song` already resets this.
    cpu.registers.status = Status::empty();
    setup_stack_for_rts(&mut cpu);

    #[allow(clippy::cast_possible_truncation)]
    let song_index = song.saturating_sub(1) as u8;
    cpu.registers.accumulator = song_index;
    cpu.registers.program_counter = sid_file.init_address;
    cpu
}

fn setup_stack_for_rts(cpu: &mut CPU<C64Memory, Nmos6502>) {
    // Tunes expect JSR/RTS pairing; plant an RTS at $0000 and a $FFFF return on the stack.
    cpu.memory.set_byte(SENTINEL_PC, RTS_OPCODE);
    reset_stack_pointer(cpu);
}

fn reset_stack_pointer(cpu: &mut CPU<C64Memory, Nmos6502>) {
    cpu.memory.set_byte(STACK_HIGH, STACK_SENTINEL);
    cpu.memory.set_byte(STACK_LOW, STACK_SENTINEL);
    cpu.registers.stack_pointer = StackPointer(STACK_POINTER_INIT);
}

fn mix_sample(sum: i32, sid_count: usize) -> f32 {
    #[allow(clippy::cast_precision_loss)]
    let mixed = (sum as f32) / (sid_count as f32) / 32768.0;
    // Keep a small headroom: backends running at i16/u16 will wrap if a sample
    // converts to exactly i16::MAX, and downstream limiters dislike a hot 1.0.
    mixed.clamp(-0.999_5, 0.999_5)
}

/// Converts a SID oscillator frequency register value to a MIDI note number.
fn freq_to_note(freq: u16, clock_hz: u32) -> u8 {
    let hz = f32::from(freq) * clock_hz as f32 / 16_777_216.0;
    if hz <= 0.0 {
        return 0;
    }
    let midi = 69.0 + 12.0 * (hz / 440.0).log2();
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    {
        midi.round().clamp(0.0, 127.0) as u8
    }
}

/// Runs the tune's init routine.
///
/// PSID init code is expected to `RTS` back to the sentinel; hitting the step
/// limit there means the tune is broken or needs hardware we don't emulate, so
/// it is an error. RSID (and KERNAL-emulation PSID) tunes often use their init
/// as the main loop and never return — the playback IRQ keeps running from that
/// point, so a timeout is accepted instead of failing the load.
fn run_init(
    cpu: &mut CPU<C64Memory, Nmos6502>,
    init_address: u16,
    tolerate_timeout: bool,
) -> PlayerResult<bool> {
    let result = run_routine(
        cpu,
        1_000_000,
        PlayerError::InitTimeout {
            steps: 1_000_000,
            address: init_address,
        },
    );
    match result {
        Ok(()) => Ok(true),
        Err(PlayerError::InitTimeout { .. }) if tolerate_timeout => Ok(false),
        Err(e) => Err(e),
    }
}

/// Runs the CPU synchronously until PC reaches `SENTINEL_PC` or the step limit is hit.
/// Used only for the init routine; in-flight `fill_buffer` uses cycle-accurate stepping.
fn run_routine(
    cpu: &mut CPU<C64Memory, Nmos6502>,
    max_steps: u32,
    timeout_err: PlayerError,
) -> PlayerResult<()> {
    let mut steps = 0;
    while steps < max_steps {
        if cpu.registers.program_counter == SENTINEL_PC {
            return Ok(());
        }
        let prev_cycles = cpu.cycles;
        cpu.single_step();
        let elapsed = (cpu.cycles - prev_cycles).max(1);
        cpu.memory.tick(elapsed);
        steps += 1;
    }
    Err(timeout_err)
}

/// Thread-safe handle for sharing the player between audio and UI threads.
pub type SharedPlayer = Arc<Mutex<Player>>;

/// Creates a player wrapped for thread-safe sharing.
pub fn create_shared_player(
    sid_file: &SidFile,
    song: u16,
    sample_rate: u32,
    chip_override: Option<ChipModel>,
    sampling_method: SamplingMethod,
) -> PlayerResult<SharedPlayer> {
    Player::new(sid_file, song, sample_rate, chip_override, sampling_method)
        .map(|p| Arc::new(Mutex::new(p)))
}

/// Locks the player for a UI action, recovering a poisoned mutex instead of
/// making every button silently stop working after one bad tune panics.
pub fn lock_player(player: &SharedPlayer) -> std::sync::MutexGuard<'_, Player> {
    player
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Non-blocking variant for the audio thread and per-frame visualizers.
/// `None` means the lock is genuinely busy, not poisoned.
pub fn try_lock_player(player: &SharedPlayer) -> Option<std::sync::MutexGuard<'_, Player>> {
    match player.try_lock() {
        Ok(guard) => Some(guard),
        Err(std::sync::TryLockError::Poisoned(poisoned)) => Some(poisoned.into_inner()),
        Err(std::sync::TryLockError::WouldBlock) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a tiny tune loaded at $0800 with the given entry points and machine code.
    fn synthetic_sid(magic: &str, init: u16, play: u16, data: Vec<u8>) -> SidFile {
        SidFile {
            magic: magic.to_string(),
            version: 2,
            data_offset: 0x76,
            load_address: 0x0800,
            init_address: init,
            play_address: play,
            songs: 1,
            start_song: 1,
            speed: 0,
            name: "test".to_string(),
            author: "test".to_string(),
            released: "test".to_string(),
            flags: 0, // PAL
            data,
            md5: String::new(),
            second_sid_address: None,
            third_sid_address: None,
        }
    }

    /// A do-nothing PSID tune: both init and play are a single RTS at $0800.
    fn frame_psid() -> SidFile {
        synthetic_sid("PSID", 0x0800, 0x0800, vec![RTS_OPCODE])
    }

    #[test]
    fn zero_cycle_instruction_still_advances_clock() {
        let sid = frame_psid();
        let mut p = Player::new(&sid, 1, 44_100, None, SamplingMethod::Fast).unwrap();

        // JAM halts mos6502: the opcode itself costs 2 cycles, but every later
        // single_step() costs 0. `advance_one_step` must still charge a cycle so
        // the audio loop cannot spin forever waiting for the VIC/CIA to tick.
        p.cpu.memory.set_byte(0x0900, 0x02);
        p.cpu.registers.program_counter = 0x0900;
        assert!(p.cpu.single_step(), "JAM should execute once");
        let before = p.cpu.cycles;
        assert!(p.advance_one_step());
        assert_eq!(p.cpu.cycles, before + 1);
    }

    #[test]
    fn rsid_vic_irq_handler_is_dispatched() {
        // Init at $0800 installs a VIC raster IRQ handler through the KERNAL user
        // vector and arms $D012/$D01A. The handler at $0820 acknowledges the IRQ,
        // opens SID voice 1 and RTIs back to the sentinel.
        #[rustfmt::skip]
        let mut data = vec![
            0xA9, 0x20,       // LDA #$20
            0x8D, 0x14, 0x03, // STA $0314
            0xA9, 0x08,       // LDA #$08
            0x8D, 0x15, 0x03, // STA $0315
            0xA9, 0x00,       // LDA #$00
            0x8D, 0x12, 0xD0, // STA $D012  (raster compare = line 0)
            0xA9, 0x01,       // LDA #$01
            0x8D, 0x1A, 0xD0, // STA $D01A  (enable raster IRQ)
            0x60,             // RTS
        ];
        data.resize(0x20, 0); // handler starts at $0820
        data.extend_from_slice(&[
            0xEE, 0x00, 0x02, // INC $0200  (count handler entries)
            0xA9, 0x01, // LDA #$01
            0x8D, 0x19, 0xD0, // STA $D019  (acknowledge raster IRQ)
            0xA9, 0x21, // LDA #$21
            0x8D, 0x00, 0xD4, // STA $D400  (voice 1 frequency low)
            0xA9, 0x11, // LDA #$11
            0x8D, 0x04, 0xD4, // STA $D404  (gate + triangle)
            0x68, 0xA8, // PLA; TAY  (restore the registers $FF48 saved)
            0x68, 0xAA, // PLA; TAX
            0x68, // PLA
            0x40, // RTI
        ]);

        let sid = synthetic_sid("RSID", 0x0800, 0, data);
        let mut p = Player::new(&sid, 1, 44_100, None, SamplingMethod::Fast).unwrap();
        assert_eq!(p.drive_mode, DriveMode::Interrupt);
        assert!(!p.free_running, "init should have returned to the sentinel");
        assert_eq!(
            p.cpu.memory.get_byte(0xD01A) & 0x01,
            1,
            "raster IRQ should be enabled"
        );

        let mut buffer = [0.0f32; 1024];
        for _ in 0..4 {
            p.fill_buffer(&mut buffer);
            assert!(p.playback_error.is_none(), "{:?}", p.playback_error);
        }

        let entries = p.cpu.memory.get_byte(0x0200);
        assert!(entries > 0, "VIC IRQ handler never ran");
        let state = p.cpu.memory.sids[0].sid.read_state();
        assert_eq!(
            state.sid_register[0x04] & 0x11,
            0x11,
            "IRQ handler did not open a SID voice"
        );
    }

    #[test]
    fn rewriting_the_current_raster_line_does_not_double_play_calls() {
        // Real VIC-II evaluates the raster compare once per line. A handler that
        // ACKs $D019 and then writes the *same* $D012 value must not re-trigger
        // itself before RTI — doing so ran the sequencer 2-3x per frame in some
        // RSID tunes (e.g. Myth and Arkanoid).
        #[rustfmt::skip]
        let mut data = vec![
            0xA9, 0x20,       // LDA #$20
            0x8D, 0x14, 0x03, // STA $0314
            0xA9, 0x08,       // LDA #$08
            0x8D, 0x15, 0x03, // STA $0315
            0xA9, 0x00,       // LDA #$00
            0x8D, 0x12, 0xD0, // STA $D012  (raster compare = line 0)
            0xA9, 0x01,       // LDA #$01
            0x8D, 0x1A, 0xD0, // STA $D01A  (enable raster IRQ)
            0x60,             // RTS
        ];
        data.resize(0x20, 0);
        data.extend_from_slice(&[
            0xEE, 0x00, 0x02, // INC $0200  (count handler entries)
            0xA9, 0x01, // LDA #$01
            0x8D, 0x19, 0xD0, // STA $D019  (acknowledge raster IRQ)
            0xA9, 0x00, // LDA #$00
            0x8D, 0x12, 0xD0, // STA $D012  (rewrite the current compare line)
            0x68, 0xA8, // PLA; TAY
            0x68, 0xAA, // PLA; TAX
            0x68, // PLA
            0x40, // RTI
        ]);

        let sid = synthetic_sid("RSID", 0x0800, 0, data);
        let mut p = Player::new(&sid, 1, 44_100, None, SamplingMethod::Fast).unwrap();

        let frames = 3u32;
        let target = PAL_FRAME_CYCLES * frames;
        while p.frame_cycle_count < target {
            assert!(p.advance_one_step(), "{:?}", p.playback_error);
        }

        let entries = p.cpu.memory.get_byte(0x0200);
        assert!(
            (1..=frames as u8 + 1).contains(&entries),
            "expected about one play call per frame, got {entries} in {frames} frames"
        );
    }

    #[test]
    fn rsid_handler_that_decs_d019_runs_once_per_frame() {
        // `DEC $D019` is a common way to acknowledge the raster IRQ: the 6502
        // read-modify-write cycle performs a dummy write of the value read
        // ($F1), which clears the flag on real hardware. The CPU emulator only
        // issues the final write ($F0), so the VIC has to infer the dummy write.
        // Without that, the flag stays set and the handler runs back-to-back at
        // CPU speed instead of once per frame (heard as a tune playing too fast,
        // e.g. Great Giana Sisters subsongs 2+).
        #[rustfmt::skip]
        let mut data = vec![
            0xA9, 0x20,       // LDA #$20
            0x8D, 0x14, 0x03, // STA $0314
            0xA9, 0x08,       // LDA #$08
            0x8D, 0x15, 0x03, // STA $0315
            0xA9, 0x00,       // LDA #$00
            0x8D, 0x12, 0xD0, // STA $D012  (raster compare = line 0)
            0xA9, 0x01,       // LDA #$01
            0x8D, 0x1A, 0xD0, // STA $D01A  (enable raster IRQ)
            0x60,             // RTS
        ];
        data.resize(0x20, 0);
        data.extend_from_slice(&[
            0xEE, 0x00, 0x02, // INC $0200 (count low byte)
            0xD0, 0x03,       // BNE +3
            0xEE, 0x01, 0x02, // INC $0201 (count high byte)
            0xCE, 0x19, 0xD0, // DEC $D019 (acknowledge raster IRQ)
            0x68, 0xA8, // PLA; TAY
            0x68, 0xAA, // PLA; TAX
            0x68, // PLA
            0x40, // RTI
        ]);

        let sid = synthetic_sid("RSID", 0x0800, 0, data);
        let mut p = Player::new(&sid, 1, 44_100, None, SamplingMethod::Fast).unwrap();
        assert_eq!(p.drive_mode, DriveMode::Interrupt);

        let frames = 3u32;
        let target = PAL_FRAME_CYCLES * frames;
        while p.frame_cycle_count < target {
            assert!(p.advance_one_step(), "{:?}", p.playback_error);
        }

        let entries = u16::from(p.cpu.memory.get_byte(0x0200))
            | (u16::from(p.cpu.memory.get_byte(0x0201)) << 8);
        assert!(
            (1..=frames as u16 + 1).contains(&entries),
            "expected about one handler call per frame, got {entries} in {frames} frames"
        );
    }

    #[test]
    fn interrupt_driven_tune_with_long_main_loop_is_not_killed_by_runaway_guard() {
        // Some RSIDs (e.g. Savage) return from init but then spend long stretches
        // in their own play/main loop without returning to the $0000 sentinel.
        // The runaway guard is for frame/CIA tunes that must return from play(),
        // so it must not fire merely because CPU time accumulates between IRQs.
        #[rustfmt::skip]
        let mut data = vec![
            0xA9, 0x20,       // LDA #$20
            0x8D, 0x14, 0x03, // STA $0314
            0xA9, 0x08,       // LDA #$08
            0x8D, 0x15, 0x03, // STA $0315
            0xA9, 0x00,       // LDA #$00
            0x8D, 0x12, 0xD0, // STA $D012  (raster compare = line 0)
            0xA9, 0x01,       // LDA #$01
            0x8D, 0x1A, 0xD0, // STA $D01A  (enable raster IRQ)
            0x60,             // RTS
        ];
        data.resize(0x20, 0); // IRQ handler at $0820
        data.extend_from_slice(&[
            0xEE, 0x00, 0x02, // INC $0200  (count handler entries)
            0xA9, 0x01, // LDA #$01
            0x8D, 0x19, 0xD0, // STA $D019  (acknowledge raster IRQ)
            0x68, 0xA8, // PLA; TAY
            0x68, 0xAA, // PLA; TAX
            0x68, // PLA
            0x40, // RTI
        ]);
        data.resize(0x30, 0); // main loop at $0830
        data.extend_from_slice(&[
            0x4C, 0x30, 0x08, // JMP $0830
        ]);

        let sid = synthetic_sid("RSID", 0x0800, 0, data);
        let mut p = Player::new(&sid, 1, 44_100, None, SamplingMethod::Fast).unwrap();
        assert_eq!(p.drive_mode, DriveMode::Interrupt);
        assert!(!p.free_running, "init should have returned to the sentinel");

        // Init returned to the sentinel; emulate the tune's own main program
        // taking over from there, as Savage's copied player does.
        p.cpu.registers.program_counter = 0x0830;

        let mut buffer = [0.0f32; 1024];
        for _ in 0..30 {
            p.fill_buffer(&mut buffer);
            assert!(p.playback_error.is_none(), "{:?}", p.playback_error);
            assert!(!p.is_paused());
        }
        assert!(
            p.cpu.memory.get_byte(0x0200) > 0,
            "raster IRQ handler never ran"
        );
    }

    #[test]
    fn loading_another_file_clears_stale_ram_from_previous_tune() {
        // A tune that used $0200 as a work area must not leave that byte behind
        // for the next file: stale RAM made Savage fall silent after switching
        // away to ACE II and back.
        let first = synthetic_sid("PSID", 0x0800, 0x0800, vec![0x60]);
        let second = synthetic_sid("PSID", 0x0800, 0x0800, vec![0x60]);
        let mut p = Player::new(&first, 1, 44_100, None, SamplingMethod::Fast).unwrap();

        p.cpu.memory.set_byte(0x0200, 0xA5);
        p.load_sid_file(&second, 1).unwrap();

        assert_eq!(p.cpu.memory.get_byte(0x0200), 0);
    }

    #[test]
    fn cia2_and_vic_register_writes_during_init_do_not_fail_load() {
        #[rustfmt::skip]
        let data = vec![
            0xA9, 0x80,       // LDA #$80
            0x8D, 0x0D, 0xDD, // STA $DD0D  (CIA2 ICR)
            0xA9, 0x01,       // LDA #$01
            0x8D, 0x0E, 0xDD, // STA $DD0E  (CIA2 CRA)
            0x8D, 0x1A, 0xD0, // STA $D01A  (VIC IMR)
            0x8D, 0x19, 0xD0, // STA $D019  (VIC flag/ack)
            0x60,             // RTS
        ];
        let sid = synthetic_sid("RSID", 0x0800, 0, data);
        // Before the VIC/CIA2 support this returned UnsupportedHardware.
        let mut p = Player::new(&sid, 1, 44_100, None, SamplingMethod::Fast).unwrap();
        assert_eq!(p.cpu.memory.get_byte(0xDD0D), 0x00);
        assert_eq!(p.cpu.memory.get_byte(0xDD0E), 0x01);
    }

    #[test]
    fn free_running_rsid_still_services_irqs_and_ea31_acks_cia1() {
        // Init installs a $0314 CIA1 Timer A handler that exits with JMP $EA31,
        // enables CIA1 Timer A IRQ, clears I flag, then loops forever (`JMP *`).
        #[rustfmt::skip]
        let mut data = vec![
            0x78,             // SEI
            0xA9, 0x30,       // LDA #$30
            0x8D, 0x14, 0x03, // STA $0314
            0xA9, 0x08,       // LDA #$08
            0x8D, 0x15, 0x03, // STA $0315
            0xA9, 0x81,       // LDA #$81 (enable Timer A IRQ)
            0x8D, 0x0D, 0xDC, // STA $DC0D
            0x58,             // CLI
            0x4C, 0x11, 0x08, // JMP $0811 (free-running wait loop)
        ];
        data.resize(0x30, 0);
        data.extend_from_slice(&[
            0xEE, 0x00, 0x02, // INC $0200 (count IRQ entries)
            0x4C, 0x31, 0xEA, // JMP $EA31 (KERNAL exit must ACK $DC0D)
        ]);

        let sid = synthetic_sid("RSID", 0x0800, 0, data);
        let mut p = Player::new(&sid, 1, 44_100, None, SamplingMethod::Fast).unwrap();
        assert!(p.free_running, "init loop should mark tune as free_running");

        let mut buffer = vec![0.0f32; 4096];
        for _ in 0..5 {
            p.fill_buffer(&mut buffer);
            assert!(p.playback_error.is_none(), "{:?}", p.playback_error);
        }

        let entries = p.cpu.memory.get_byte(0x0200);
        // ~0.46s at ~60Hz => ~27 IRQs; if $EA31 did not ACK $DC0D it would fire hundreds of times.
        assert!(
            (10..=60).contains(&entries),
            "expected normal ~60Hz CIA1 IRQ rate, got {entries}"
        );
    }

    #[test]
    fn cia2_timer_a_triggers_nmi_through_0318_vector() {
        // Init installs an NMI handler at $0318 ($0830), programs CIA2 Timer A
        // to fire every ~1000 cycles, enables NMI, and returns via RTS.
        #[rustfmt::skip]
        let mut data = vec![
            0xA9, 0x30,       // LDA #$30
            0x8D, 0x18, 0x03, // STA $0318
            0xA9, 0x08,       // LDA #$08
            0x8D, 0x19, 0x03, // STA $0319
            0xA9, 0xE8,       // LDA #$E8
            0x8D, 0x04, 0xDD, // STA $DD04 (latch lo = 1000)
            0xA9, 0x03,       // LDA #$03
            0x8D, 0x05, 0xDD, // STA $DD05 (latch hi)
            0xA9, 0x81,       // LDA #$81
            0x8D, 0x0D, 0xDD, // STA $DD0D (enable Timer A NMI)
            0xA9, 0x11,       // LDA #$11
            0x8D, 0x0E, 0xDD, // STA $DD0E (force-load & start)
            0x60,             // RTS
        ];
        data.resize(0x30, 0);
        data.extend_from_slice(&[
            0xEE, 0x00, 0x02, // INC $0200 (count NMI entries, low byte)
            0xD0, 0x03,       // BNE skip-high (skip the next INC)
            0xEE, 0x01, 0x02, // INC $0201 (high byte)
            0xAD, 0x0D, 0xDD, // LDA $DD0D (ACK CIA2 NMI)
            0x40,             // RTI
        ]);

        let sid = synthetic_sid("RSID", 0x0800, 0, data);
        let mut p = Player::new(&sid, 1, 44_100, None, SamplingMethod::Fast).unwrap();

        let mut buffer = vec![0.0f32; 4096];
        p.fill_buffer(&mut buffer);
        assert!(p.playback_error.is_none(), "{:?}", p.playback_error);

        let lo = u16::from(p.cpu.memory.get_byte(0x0200));
        let hi = u16::from(p.cpu.memory.get_byte(0x0201));
        let nmi_entries = lo | (hi << 8);
        // ~91k emulated cycles at 1001 cycles per underflow. A second, duplicate
        // service from mos6502's poller would roughly double this count.
        assert!(
            (70..=110).contains(&nmi_entries),
            "expected one CIA2 Timer A NMI per underflow, got {nmi_entries}"
        );
    }

    #[test]
    fn lock_helpers_recover_a_poisoned_player() {
        let sid = frame_psid();
        let shared = create_shared_player(&sid, 1, 44_100, None, SamplingMethod::Fast).unwrap();
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = shared.lock().unwrap();
            panic!("simulate a panic inside the audio callback");
        }));
        assert!(shared.is_poisoned());

        // UI actions and the audio callback must keep working after a bad tune
        // panicked once; otherwise every button silently stops responding.
        assert!(try_lock_player(&shared).is_some());
        assert!(lock_player(&shared).playback_error.is_none());
    }
}
