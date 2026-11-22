// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2026 Emarti
//
// Portions derived from crabsid by Mikael Lund,
// Copyright (c) 2026 Mikael Lund, licensed under the MIT License.
// See licenses/MIT-crabsid.txt for the original license text.

//! Emulated C64 memory map: RAM, banked KERNAL stub, SID chips, CIA1, VIC-II.

use crate::engine::cia::Cia1;
use crate::engine::sid_file::PRIMARY_SID_ADDRESS;
use crate::engine::vic::Vic;
use mos6502::memory::Bus;
use residfp::{ChipModel, Sid};

const RAM_SIZE: usize = 65536;
const ZEROPAGE_AND_STACK_END: usize = 0x0200;
const SID_REGISTER_COUNT: u16 = 0x20;
const SID_RANGE_START: u16 = 0xD400;
const SID_RANGE_END: u16 = 0xD800;

// VIC-II register window ($D000..$D400, mirrored every 64 bytes).
const VIC_BASE: u16 = 0xD000;
const VIC_END: u16 = 0xD400;

// Banking control registers in zero page.
const BANK_DATA_DIR_ADDR: u16 = 0x0000;
const BANK_CONFIG_ADDR: u16 = 0x0001;
/// Default `$00` data direction (LORAM/HIRAM/CHAREN bits + tape lines as outputs).
const BANK_DATA_DIR_DEFAULT: u8 = 0x2F;
/// Default `$01`: KERNAL + BASIC + I/O all banked in.
const BANK_DEFAULT: u8 = 0x37;
/// HIRAM bit gates KERNAL ROM at `$E000-$FFFF`. LORAM (BASIC) and CHAREN (CHARROM)
/// are deliberately ignored — RSID files driven by CIA1 IRQs only need the KERNAL
/// IRQ trampoline; pulling extra ROMs in would require redistributable ROM images.
const BANK_HIRAM_BIT: u8 = 1 << 1;

// KERNAL stub region.
const KERNAL_BASE: u16 = 0xE000;
const KERNAL_SIZE: usize = 0x2000;

// Stub-internal addresses where we patch specific opcodes/vectors.
const KERNAL_IRQ_ENTRY: u16 = 0xFF48;
const KERNAL_IRQ_EXIT: u16 = 0xFF8E;
/// Standard KERNAL IRQ-exit entry points. Many RSID handlers finish with
/// `JMP $EA31` (full keyboard scan) or `JMP $EA7E`/`$EA81` (skip scan) and
/// expect the KERNAL to restore registers and `RTI`. The stub turns all three
/// into a plain `RTI`; SID playback does not need the keyboard scan.
const KERNAL_IRQ_EXITS: [u16; 3] = [0xEA31, 0xEA7E, 0xEA81];
const HW_IRQ_VECTOR_LO: u16 = 0xFFFE;
const HW_IRQ_VECTOR_HI: u16 = 0xFFFF;
const HW_NMI_VECTOR_LO: u16 = 0xFFFA;
const HW_NMI_VECTOR_HI: u16 = 0xFFFB;

/// User-installed IRQ vector in RAM. Tunes overwrite this in init to point at their
/// handler; default points at the stub's RTI so pre-init IRQs are silent.
const USER_IRQ_VECTOR_LO: u16 = 0x0314;
const USER_IRQ_VECTOR_HI: u16 = 0x0315;

const OPCODE_JMP_INDIRECT: u8 = 0x6C;
const OPCODE_RTI: u8 = 0x40;
/// Default fill byte for the KERNAL stub. RTS keeps stray calls from corrupting
/// the stack; only the IRQ exit address is RTI.
const OPCODE_RTS: u8 = 0x60;

// CIA1 register window (mirrored across $DC00..=$DCFF).
const CIA1_BASE: u16 = 0xDC00;
const CIA1_END: u16 = 0xDD00;

// CIA2 register window (mirrored across $DD00..=$DDFF).
const CIA2_BASE: u16 = 0xDD00;
const CIA2_END: u16 = 0xDE00;

const KERNAL_STUB: [u8; KERNAL_SIZE] = build_kernel_stub();

const fn build_kernel_stub() -> [u8; KERNAL_SIZE] {
    let mut stub = [OPCODE_RTS; KERNAL_SIZE];
    let [entry_lo, entry_hi] = KERNAL_IRQ_ENTRY.to_le_bytes();
    let [user_lo, user_hi] = USER_IRQ_VECTOR_LO.to_le_bytes();

    // Hardware IRQ + NMI vectors → KERNAL IRQ entry. We don't service NMI
    // separately, but the vector cannot be 0 or random ROM byte.
    stub[(HW_IRQ_VECTOR_LO - KERNAL_BASE) as usize] = entry_lo;
    stub[(HW_IRQ_VECTOR_HI - KERNAL_BASE) as usize] = entry_hi;
    stub[(HW_NMI_VECTOR_LO - KERNAL_BASE) as usize] = entry_lo;
    stub[(HW_NMI_VECTOR_HI - KERNAL_BASE) as usize] = entry_hi;

    // $FF48: JMP ($0314) — trampoline through user IRQ vector.
    let entry = (KERNAL_IRQ_ENTRY - KERNAL_BASE) as usize;
    stub[entry] = OPCODE_JMP_INDIRECT;
    stub[entry + 1] = user_lo;
    stub[entry + 2] = user_hi;

    // $FF8E: RTI — exit point for handlers that JMP here to return.
    stub[(KERNAL_IRQ_EXIT - KERNAL_BASE) as usize] = OPCODE_RTI;

    // Standard KERNAL IRQ exits: let handlers that JMP into the ROM finish the
    // interrupt instead of executing the stub's default RTS and losing the
    // interrupt frame.
    let mut i = 0;
    while i < KERNAL_IRQ_EXITS.len() {
        stub[(KERNAL_IRQ_EXITS[i] - KERNAL_BASE) as usize] = OPCODE_RTI;
        i += 1;
    }

    stub
}

/// Control-register bits kept while a voice is muted: sync, ring-mod and test.
/// Clearing the gate bit lets the envelope release to zero and clearing the
/// waveform nibble silences the oscillator, while the oscillator itself keeps
/// clocking so hard-sync relationships with the other voices stay intact.
const MUTE_CONTROL_MASK: u8 = 0x0E;

/// A SID chip with its base memory address.
pub struct SidChip {
    pub sid: Sid,
    pub base_address: u16,
    /// Per-voice mute switches, driven by the mixer UI.
    voice_mute: [bool; 3],
    /// Last control value the tune wrote per voice. While a voice is muted the
    /// chip receives a masked copy; unmuting writes the intended value back so
    /// a held note starts sounding again.
    voice_control: [u8; 3],
}

impl SidChip {
    pub fn new(chip_model: ChipModel, base_address: u16) -> Self {
        Self {
            sid: Sid::new(chip_model),
            base_address,
            voice_mute: [false; 3],
            voice_control: [0; 3],
        }
    }

    fn contains(&self, addr: u16) -> bool {
        addr >= self.base_address && addr < self.base_address + SID_REGISTER_COUNT
    }

    /// Silences or restores one voice (0-2) without stopping the chip.
    pub fn set_voice_muted(&mut self, voice: usize, muted: bool) {
        if voice >= 3 || self.voice_mute[voice] == muted {
            return;
        }
        self.voice_mute[voice] = muted;
        #[allow(clippy::cast_possible_truncation)]
        let reg = (voice * 7 + 4) as u8;
        let value = if muted {
            self.voice_control[voice] & MUTE_CONTROL_MASK
        } else {
            self.voice_control[voice]
        };
        self.sid.write(reg, value);
    }

    pub fn voice_muted(&self, voice: usize) -> bool {
        self.voice_mute.get(voice).copied().unwrap_or(false)
    }

    /// Drops the remembered control values after the chip is reset, so a stale
    /// value cannot be restored by a later unmute.
    pub const fn forget_voice_controls(&mut self) {
        self.voice_control = [0; 3];
    }

    /// Register write that masks the control register of a muted voice.
    fn write(&mut self, reg: u8, val: u8) {
        if reg % 7 == 4 {
            let voice = (reg / 7) as usize;
            if voice < 3 {
                self.voice_control[voice] = val;
                let applied = if self.voice_mute[voice] {
                    val & MUTE_CONTROL_MASK
                } else {
                    val
                };
                self.sid.write(reg, applied);
                return;
            }
        }
        self.sid.write(reg, val);
    }
}

/// Emulated C64 memory map.
///
/// Hosts 64KB RAM, 1-3 SID chips, CIA1 (Timer A + ICR), VIC-II (raster IRQ), CIA2 registers,
/// an 8KB KERNAL stub, and `$00/$01` HIRAM banking.
pub struct C64Memory {
    ram: Box<[u8]>,
    pub sids: Vec<SidChip>,
    pub cia1: Cia1,
    pub vic: Vic,
    cia2_regs: [u8; 16],
    bank_config: u8,
    /// Whether hardware IRQ lines (CIA1 or VIC-II) are routed to the CPU. Off by default
    /// so PSID tunes (frame or CIA-poll) never have mos6502 service spurious IRQs;
    /// `Player` flips it on for RSID-style tunes that expect vector dispatch.
    irq_routed: bool,
}

impl C64Memory {
    /// Creates memory with a single SID chip at `$D400` and C64 default banking.
    pub fn new(chip_model: ChipModel) -> Self {
        let mut mem = Self {
            ram: vec![0; RAM_SIZE].into_boxed_slice(),
            sids: vec![SidChip::new(chip_model, PRIMARY_SID_ADDRESS)],
            cia1: Cia1::new(),
            vic: Vic::default(),
            cia2_regs: [0; 16],
            bank_config: BANK_DEFAULT,
            irq_routed: false,
        };
        mem.init_zeropage_defaults();
        mem
    }

    /// Configures SID chips from `(base_address, chip_model)` pairs.
    /// First entry should always be `$D400` for the primary SID.
    pub fn configure_sids(&mut self, configs: &[(u16, ChipModel)]) {
        self.sids = configs
            .iter()
            .map(|&(addr, model)| SidChip::new(model, addr))
            .collect();
    }

    /// Loads binary data into RAM at the specified address.
    pub fn load(&mut self, address: u16, data: &[u8]) {
        let start = address as usize;
        let end = (start + data.len()).min(RAM_SIZE);
        self.ram[start..end].copy_from_slice(&data[..end - start]);
    }

    /// Resets zero page + stack and re-arms banking, CIA1, VIC-II, and the IRQ vector.
    /// Called between songs so init starts from a clean machine state.
    pub fn clear_zeropage_and_stack(&mut self) {
        self.ram[..ZEROPAGE_AND_STACK_END].fill(0);
        self.bank_config = BANK_DEFAULT;
        self.cia1 = Cia1::new();
        self.vic.reset();
        self.cia2_regs.fill(0);
        self.init_zeropage_defaults();
    }

    fn init_zeropage_defaults(&mut self) {
        let [exit_lo, exit_hi] = KERNAL_IRQ_EXIT.to_le_bytes();
        self.ram[BANK_DATA_DIR_ADDR as usize] = BANK_DATA_DIR_DEFAULT;
        self.ram[USER_IRQ_VECTOR_LO as usize] = exit_lo;
        self.ram[USER_IRQ_VECTOR_HI as usize] = exit_hi;
    }

    /// Replace the chip model for a specific SID (by index).
    pub fn set_chip_model(&mut self, index: usize, chip_model: ChipModel) {
        if let Some(sid_chip) = self.sids.get_mut(index) {
            let base = sid_chip.base_address;
            // Keep the mixer switches and last control values across the swap:
            // the caller re-applies the register snapshot right after.
            let mute = sid_chip.voice_mute;
            let control = sid_chip.voice_control;
            *sid_chip = SidChip::new(chip_model, base);
            sid_chip.voice_mute = mute;
            sid_chip.voice_control = control;
        }
    }

    /// Per-voice mute switches, ordered chip by chip (3 entries per SID).
    pub fn voice_mutes(&self) -> Vec<bool> {
        self.sids
            .iter()
            .flat_map(|chip| (0..3).map(|voice| chip.voice_muted(voice)))
            .collect()
    }

    /// Silences or restores one voice by its global index (chip * 3 + voice).
    pub fn set_voice_muted(&mut self, index: usize, muted: bool) {
        if let Some(chip) = self.sids.get_mut(index / 3) {
            chip.set_voice_muted(index % 3, muted);
        }
    }

    /// Advances per-cycle peripherals (CIA1, VIC-II) by `cycles`.
    pub fn tick(&mut self, cycles: u64) {
        self.cia1.tick(cycles);
        self.vic.tick(cycles);
    }

    /// Configures the video standard for VIC-II (PAL vs NTSC).
    pub fn set_pal(&mut self, is_pal: bool) {
        self.vic.set_pal(is_pal);
    }

    /// PSID-CIA poll: did Timer A underflow since last call? Clears the flag.
    pub fn cia1_take_timer_a_underflow(&mut self) -> bool {
        self.cia1.take_timer_a_underflow()
    }

    /// Toggles whether hardware IRQ lines (CIA1 or VIC-II) drive `Bus::irq_pending`.
    /// Only RSID-style tunes want this — PSID-CIA polls the underflow flag directly via
    /// `cia1_take_timer_a_underflow` and would be disrupted by mos6502 servicing.
    pub fn set_irq_routed(&mut self, routed: bool) {
        self.irq_routed = routed;
    }

    /// Backward compatibility alias for [`set_irq_routed`].
    pub fn set_cia_irq_routed(&mut self, routed: bool) {
        self.set_irq_routed(routed);
    }

    /// Forces the `$01` banking byte. Used by the player to apply the PSID-driver
    /// `iomap` value before init/play, so tunes whose code lives in the KERNAL
    /// or BASIC ROM area execute from RAM instead of the banked-in stub.
    pub const fn set_bank_config(&mut self, value: u8) {
        self.bank_config = value;
    }

    fn kernal_banked_in(&self) -> bool {
        self.bank_config & BANK_HIRAM_BIT != 0
    }
}

impl Bus for C64Memory {
    fn get_byte(&mut self, addr: u16) -> u8 {
        if addr == BANK_CONFIG_ADDR {
            return self.bank_config;
        }
        if addr >= KERNAL_BASE && self.kernal_banked_in() {
            return KERNAL_STUB[(addr - KERNAL_BASE) as usize];
        }
        if (VIC_BASE..VIC_END).contains(&addr) {
            #[allow(clippy::cast_possible_truncation)]
            return self.vic.read_register((addr & 0x3F) as u8);
        }
        if (SID_RANGE_START..SID_RANGE_END).contains(&addr) {
            for sid_chip in &mut self.sids {
                if sid_chip.contains(addr) {
                    #[allow(clippy::cast_possible_truncation)]
                    return sid_chip.sid.read((addr - sid_chip.base_address) as u8);
                }
            }
        }
        if (CIA1_BASE..CIA1_END).contains(&addr) {
            #[allow(clippy::cast_possible_truncation)]
            return self.cia1.read_register((addr & 0x0F) as u8);
        }
        if (CIA2_BASE..CIA2_END).contains(&addr) {
            let reg = (addr & 0x0F) as usize;
            if reg == 0x0D {
                return 0x00; // CIA2 ICR read returns 0 (no pending NMIs)
            }
            return self.cia2_regs[reg];
        }
        self.ram[addr as usize]
    }

    fn set_byte(&mut self, addr: u16, val: u8) {
        if addr == BANK_CONFIG_ADDR {
            self.bank_config = val;
            return;
        }
        if (VIC_BASE..VIC_END).contains(&addr) {
            #[allow(clippy::cast_possible_truncation)]
            self.vic.write_register((addr & 0x3F) as u8, val);
            return;
        }
        if (SID_RANGE_START..SID_RANGE_END).contains(&addr) {
            for sid_chip in &mut self.sids {
                if sid_chip.contains(addr) {
                    #[allow(clippy::cast_possible_truncation)]
                    sid_chip.write((addr - sid_chip.base_address) as u8, val);
                    return;
                }
            }
        }
        if (CIA1_BASE..CIA1_END).contains(&addr) {
            #[allow(clippy::cast_possible_truncation)]
            self.cia1.write_register((addr & 0x0F) as u8, val);
            return;
        }
        if (CIA2_BASE..CIA2_END).contains(&addr) {
            self.cia2_regs[(addr & 0x0F) as usize] = val;
            return;
        }
        self.ram[addr as usize] = val;
    }

    fn irq_pending(&mut self) -> bool {
        self.irq_routed && (self.cia1.irq_line() || self.vic.irq_line())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory() -> C64Memory {
        C64Memory::new(ChipModel::Mos6581)
    }

    #[test]
    fn cia2_register_writes_are_harmless() {
        let mut mem = memory();
        // These are the registers that used to make tunes fail to load.
        mem.set_byte(0xDD0D, 0x81); // ICR / NMI flag
        mem.set_byte(0xDD0E, 0x01); // Timer A control
        assert_eq!(mem.get_byte(0xDD0D), 0x00, "no pending NMIs are reported");
        assert_eq!(
            mem.get_byte(0xDD0E),
            0x01,
            "other CIA2 registers still store writes"
        );
    }

    #[test]
    fn irq_is_not_reported_until_routed() {
        let mut mem = memory();
        // The VIC raises the raster IRQ when the beam crosses the compare line.
        mem.set_byte(0xD012, 1);
        mem.set_byte(0xD01A, 0x01);
        mem.tick(64);
        assert!(mem.vic.irq_line());
        assert!(!mem.irq_pending(), "PSID must not see hardware IRQs");

        mem.set_irq_routed(true);
        assert!(mem.irq_pending(), "RSID must see the VIC raster IRQ");
    }

    #[test]
    fn vic_and_cia1_both_feed_irq_pending() {
        let mut mem = memory();
        mem.set_irq_routed(true);
        assert!(!mem.irq_pending());

        // CIA1 Timer A underflow raises its IRQ line.
        mem.set_byte(0xDC04, 1);
        mem.set_byte(0xDC05, 0);
        mem.set_byte(0xDC0E, 0x11); // force-load latch and start Timer A
        mem.set_byte(0xDC0D, 0x81); // enable the Timer A IRQ mask
        mem.tick(2);
        assert!(mem.cia1.irq_line());
        assert!(mem.irq_pending());

        // Reading ICR clears the pending flag, as a real handler does.
        mem.get_byte(0xDC0D);
        assert!(!mem.cia1.irq_line());
        assert!(!mem.irq_pending());

        mem.set_byte(0xD012, 1);
        mem.set_byte(0xD01A, 0x01);
        mem.tick(64);
        assert!(mem.vic.irq_line());
        assert!(mem.irq_pending());
    }

    #[test]
    fn kernal_irq_exits_return_with_rti() {
        let mut mem = memory();
        for addr in [0xEA31, 0xEA7E, 0xEA81] {
            assert_eq!(mem.get_byte(addr), OPCODE_RTI, "exit at ${addr:04X}");
        }
    }
}
