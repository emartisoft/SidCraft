// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2026 Emarti
//
// Portions derived from crabsid by Mikael Lund,
// Copyright (c) 2026 Mikael Lund, licensed under the MIT License.
// See licenses/MIT-crabsid.txt for the original license text.

//! Emulated C64 memory map: RAM, banked KERNAL stub, SID chips, CIA1, VIC-II.

use crate::engine::cia::{Cia, Cia1};
use crate::engine::sid_file::PRIMARY_SID_ADDRESS;
use crate::engine::vic::Vic;
use mos6502::memory::Bus;
use residfp::{ChipModel, Sid};

const RAM_SIZE: usize = 65536;
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
const KERNAL_NMI_ENTRY: u16 = 0xFE43;
const KERNAL_NMI_EXIT: u16 = 0xFE47;
/// Real KERNAL NMI return path: pops the three registers a handler saved
/// (`PHA/TXA/PHA/TYA/PHA`) before the `RTI`. Some handlers `JMP $FEBC`.
const KERNAL_NMI_RESTORE: u16 = 0xFEBC;
/// Standard KERNAL IRQ-exit entry points. Many RSID handlers finish with
/// `JMP $EA31` (full keyboard scan) or `JMP $EA7E`/`$EA81` (skip scan).
/// On a real C64, `$EA31` and `$EA7E` read `$DC0D` (CIA1 ICR) to acknowledge
/// the timer IRQ before returning, while `$EA81` returns directly.
const KERNAL_IRQ_EXIT_EA31: u16 = 0xEA31;
const KERNAL_IRQ_EXIT_EA7E: u16 = 0xEA7E;
const KERNAL_IRQ_EXIT_EA81: u16 = 0xEA81;
const OPCODE_LDA_ABS: u8 = 0xAD;
const CIA1_ICR_LO: u8 = 0x0D;
const CIA1_ICR_HI: u8 = 0xDC;
const HW_IRQ_VECTOR_LO: u16 = 0xFFFE;
const HW_IRQ_VECTOR_HI: u16 = 0xFFFF;
const HW_NMI_VECTOR_LO: u16 = 0xFFFA;
const HW_NMI_VECTOR_HI: u16 = 0xFFFB;

/// User-installed IRQ vector in RAM. Tunes overwrite this in init to point at their
/// handler; default points at the stub's IRQ exit so pre-init IRQs are silent.
const USER_IRQ_VECTOR_LO: u16 = 0x0314;
const USER_IRQ_VECTOR_HI: u16 = 0x0315;
/// User-installed NMI vector in RAM (`$0318/$0319`).
const USER_NMI_VECTOR_LO: u16 = 0x0318;
const USER_NMI_VECTOR_HI: u16 = 0x0319;

const OPCODE_PHA: u8 = 0x48;
const OPCODE_PLA: u8 = 0x68;
const OPCODE_TAX: u8 = 0xAA;
const OPCODE_TAY: u8 = 0xA8;
const OPCODE_TXA: u8 = 0x8A;
const OPCODE_TYA: u8 = 0x98;
const OPCODE_SEI: u8 = 0x78;
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
    let [nmi_lo, nmi_hi] = KERNAL_NMI_ENTRY.to_le_bytes();
    let [user_lo, user_hi] = USER_IRQ_VECTOR_LO.to_le_bytes();
    let [user_nmi_lo, user_nmi_hi] = USER_NMI_VECTOR_LO.to_le_bytes();

    // Hardware IRQ → KERNAL IRQ entry; Hardware NMI → KERNAL NMI entry.
    stub[(HW_IRQ_VECTOR_LO - KERNAL_BASE) as usize] = entry_lo;
    stub[(HW_IRQ_VECTOR_HI - KERNAL_BASE) as usize] = entry_hi;
    stub[(HW_NMI_VECTOR_LO - KERNAL_BASE) as usize] = nmi_lo;
    stub[(HW_NMI_VECTOR_HI - KERNAL_BASE) as usize] = nmi_hi;

    // $FE43: SEI; JMP ($0318) — NMI trampoline through user NMI vector.
    // The real KERNAL does *not* save registers here; handlers do that
    // themselves (and end with `JMP $FEBC` or their own PLA/RTI).
    let nmi_entry = (KERNAL_NMI_ENTRY - KERNAL_BASE) as usize;
    stub[nmi_entry] = OPCODE_SEI;
    stub[nmi_entry + 1] = OPCODE_JMP_INDIRECT;
    stub[nmi_entry + 2] = user_nmi_lo;
    stub[nmi_entry + 3] = user_nmi_hi;
    // $FE47: RTI — default NMI handler (reached through the default vector).
    stub[(KERNAL_NMI_EXIT - KERNAL_BASE) as usize] = OPCODE_RTI;
    // $FEBC: PLA; TAY; PLA; TAX; PLA; RTI — NMI return path.
    let nmi_restore = (KERNAL_NMI_RESTORE - KERNAL_BASE) as usize;
    stub[nmi_restore] = OPCODE_PLA;
    stub[nmi_restore + 1] = OPCODE_TAY;
    stub[nmi_restore + 2] = OPCODE_PLA;
    stub[nmi_restore + 3] = OPCODE_TAX;
    stub[nmi_restore + 4] = OPCODE_PLA;
    stub[nmi_restore + 5] = OPCODE_RTI;

    // $FF48: PHA; TXA; PHA; TYA; PHA; JMP ($0314) — trampoline through user
    // IRQ vector. Saving A/X/Y here is what lets handlers finish with the
    // standard PLA/TAY/PLA/TAX/PLA/RTI sequence (or JMP $EA31).
    let entry = (KERNAL_IRQ_ENTRY - KERNAL_BASE) as usize;
    stub[entry] = OPCODE_PHA;
    stub[entry + 1] = OPCODE_TXA;
    stub[entry + 2] = OPCODE_PHA;
    stub[entry + 3] = OPCODE_TYA;
    stub[entry + 4] = OPCODE_PHA;
    stub[entry + 5] = OPCODE_JMP_INDIRECT;
    stub[entry + 6] = user_lo;
    stub[entry + 7] = user_hi;

    // $FF8E: PLA; TAY; PLA; TAX; PLA; RTI — default IRQ exit.
    let irq_exit = (KERNAL_IRQ_EXIT - KERNAL_BASE) as usize;
    stub[irq_exit] = OPCODE_PLA;
    stub[irq_exit + 1] = OPCODE_TAY;
    stub[irq_exit + 2] = OPCODE_PLA;
    stub[irq_exit + 3] = OPCODE_TAX;
    stub[irq_exit + 4] = OPCODE_PLA;
    stub[irq_exit + 5] = OPCODE_RTI;

    // $EA31: LDA $DC0D; PLA; TAY; PLA; TAX; PLA; RTI — acknowledges CIA1 ICR
    // and restores the registers saved by $FF48 before returning.
    let ea31 = (KERNAL_IRQ_EXIT_EA31 - KERNAL_BASE) as usize;
    stub[ea31] = OPCODE_LDA_ABS;
    stub[ea31 + 1] = CIA1_ICR_LO;
    stub[ea31 + 2] = CIA1_ICR_HI;
    stub[ea31 + 3] = OPCODE_PLA;
    stub[ea31 + 4] = OPCODE_TAY;
    stub[ea31 + 5] = OPCODE_PLA;
    stub[ea31 + 6] = OPCODE_TAX;
    stub[ea31 + 7] = OPCODE_PLA;
    stub[ea31 + 8] = OPCODE_RTI;

    // $EA7E: LDA $DC0D — falls through directly into $EA81.
    let ea7e = (KERNAL_IRQ_EXIT_EA7E - KERNAL_BASE) as usize;
    stub[ea7e] = OPCODE_LDA_ABS;
    stub[ea7e + 1] = CIA1_ICR_LO;
    stub[ea7e + 2] = CIA1_ICR_HI;

    // $EA81: PLA; TAY; PLA; TAX; PLA; RTI.
    let ea81 = (KERNAL_IRQ_EXIT_EA81 - KERNAL_BASE) as usize;
    stub[ea81] = OPCODE_PLA;
    stub[ea81 + 1] = OPCODE_TAY;
    stub[ea81 + 2] = OPCODE_PLA;
    stub[ea81 + 3] = OPCODE_TAX;
    stub[ea81 + 4] = OPCODE_PLA;
    stub[ea81 + 5] = OPCODE_RTI;

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
    pub cia2: Cia,
    pub vic: Vic,
    bank_config: u8,
    /// Whether hardware IRQ lines (CIA1 or VIC-II) are routed to the CPU. Off by default
    /// so PSID tunes (frame or CIA-poll) never have mos6502 service spurious IRQs;
    /// `Player` flips it on for RSID-style tunes that expect vector dispatch.
    irq_routed: bool,
    /// Set when the CPU reads CIA2's ICR, which lowers the NMI line. The player
    /// uses it to re-arm edge detection if the timer underflows again in the
    /// same instruction tick (otherwise that rising edge would be missed).
    nmi_ack_seen: bool,
}

impl C64Memory {
    /// Creates memory with a single SID chip at `$D400` and C64 default banking.
    pub fn new(chip_model: ChipModel) -> Self {
        let mut mem = Self {
            ram: vec![0; RAM_SIZE].into_boxed_slice(),
            sids: vec![SidChip::new(chip_model, PRIMARY_SID_ADDRESS)],
            cia1: Cia1::new(),
            cia2: Cia::new_stopped(),
            vic: Vic::default(),
            bank_config: BANK_DEFAULT,
            irq_routed: false,
            nmi_ack_seen: false,
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

    /// Clears all RAM and re-arms banking, CIA1, VIC-II, and the IRQ vectors.
    /// Called between songs so init starts from a clean machine state; stale RAM
    /// left by another tune can corrupt the new player's work areas.
    pub fn clear_ram(&mut self) {
        self.ram.fill(0);
        self.bank_config = BANK_DEFAULT;
        self.cia1 = Cia1::new();
        self.cia2 = Cia::new_stopped();
        self.vic.reset();
        self.nmi_ack_seen = false;
        self.init_zeropage_defaults();
    }

    fn init_zeropage_defaults(&mut self) {
        let [exit_lo, exit_hi] = KERNAL_IRQ_EXIT_EA31.to_le_bytes();
        let [nmi_exit_lo, nmi_exit_hi] = KERNAL_NMI_EXIT.to_le_bytes();
        self.ram[BANK_DATA_DIR_ADDR as usize] = BANK_DATA_DIR_DEFAULT;
        self.ram[USER_IRQ_VECTOR_LO as usize] = exit_lo;
        self.ram[USER_IRQ_VECTOR_HI as usize] = exit_hi;
        self.ram[USER_NMI_VECTOR_LO as usize] = nmi_exit_lo;
        self.ram[USER_NMI_VECTOR_HI as usize] = nmi_exit_hi;
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
    #[allow(dead_code)]
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

    /// Advances per-cycle peripherals (CIA1, CIA2, VIC-II) by `cycles`.
    pub fn tick(&mut self, cycles: u64) {
        self.cia1.tick(cycles);
        self.cia2.tick(cycles);
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

    /// Raw IRQ line state for the player's own interrupt dispatcher.
    pub fn raw_irq_pending(&mut self) -> bool {
        self.irq_routed && (self.cia1.irq_line() || self.vic.irq_line())
    }

    /// Raw NMI line state for the player's own interrupt dispatcher.
    pub fn raw_nmi_pending(&mut self) -> bool {
        self.irq_routed && self.cia2.irq_line()
    }

    /// Consumes the "CIA2 ICR was read" flag. Reading ICR lowers the NMI line;
    /// if the timer underflows again before the next instruction boundary the
    /// player must treat that as a fresh edge rather than a still-asserted line.
    pub fn take_nmi_ack_seen(&mut self) -> bool {
        std::mem::take(&mut self.nmi_ack_seen)
    }

    /// Toggles whether hardware IRQ lines (CIA1 or VIC-II) drive the player's IRQ
    /// dispatcher. Only RSID-style tunes want this — PSID-CIA polls the underflow
    /// flag directly via `cia1_take_timer_a_underflow`.
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
        // Fast path: $0002..$CFFF is always plain RAM (all I/O & KERNAL are >= $D000).
        if addr < VIC_BASE {
            return self.ram[addr as usize];
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
            if addr == 0xDD0D {
                self.nmi_ack_seen = true;
            }
            #[allow(clippy::cast_possible_truncation)]
            return self.cia2.read_register((addr & 0x0F) as u8);
        }
        self.ram[addr as usize]
    }

    fn set_byte(&mut self, addr: u16, val: u8) {
        if addr == BANK_CONFIG_ADDR {
            self.bank_config = val;
            return;
        }
        // Fast path: $0002..$CFFF is always plain RAM.
        if addr < VIC_BASE {
            self.ram[addr as usize] = val;
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
            #[allow(clippy::cast_possible_truncation)]
            self.cia2.write_register((addr & 0x0F) as u8, val);
            return;
        }
        self.ram[addr as usize] = val;
    }

    fn irq_pending(&mut self) -> bool {
        // The player is the sole interrupt dispatcher. Exposing the line to
        // mos6502 as well lets its own poller enter the handler a second time
        // with a stale return address, advancing interrupt-driven tunes twice.
        false
    }

    fn nmi_pending(&mut self) -> bool {
        // See `irq_pending`: the player consumes NMI edges via raw_nmi_pending().
        false
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
    fn clear_ram_wipes_all_ram_but_keeps_boot_defaults() {
        let mut mem = memory();
        for addr in [0x0002, 0x0200, 0x4000, 0x9FFF, 0xCFFF] {
            mem.set_byte(addr, 0xA5);
        }
        mem.set_byte(0x0314, 0x12);
        mem.set_byte(0x0315, 0x34);
        mem.set_byte(0x0318, 0x56);
        mem.set_byte(0x0319, 0x78);

        mem.clear_ram();

        for addr in [0x0002, 0x0200, 0x4000, 0x9FFF, 0xCFFF] {
            assert_eq!(
                mem.get_byte(addr),
                0,
                "stale byte at ${addr:04X} survived song change"
            );
        }
        assert_eq!(mem.get_byte(0x0001), BANK_DEFAULT);
        assert_eq!(mem.get_byte(0x0314), 0x31);
        assert_eq!(mem.get_byte(0x0315), 0xEA);
        assert_eq!(mem.get_byte(0x0318), 0x47);
        assert_eq!(mem.get_byte(0x0319), 0xFE);
    }

    #[test]
    fn irq_is_not_reported_until_routed() {
        let mut mem = memory();
        // The VIC raises the raster IRQ when the beam crosses the compare line.
        mem.set_byte(0xD012, 1);
        mem.set_byte(0xD01A, 0x01);
        mem.tick(64);
        assert!(mem.vic.irq_line());
        assert!(!mem.raw_irq_pending(), "PSID must not see hardware IRQs");

        mem.set_irq_routed(true);
        assert!(mem.raw_irq_pending(), "RSID must see the VIC raster IRQ");
    }

    #[test]
    fn bus_hides_interrupt_lines_from_mos6502() {
        let mut mem = memory();
        mem.set_irq_routed(true);
        mem.set_byte(0xD012, 1);
        mem.set_byte(0xD01A, 0x01);
        mem.tick(64);
        assert!(mem.raw_irq_pending());
        assert!(!Bus::irq_pending(&mut mem));

        mem.set_byte(0xDD04, 1);
        mem.set_byte(0xDD05, 0);
        mem.set_byte(0xDD0E, 0x11);
        mem.set_byte(0xDD0D, 0x81);
        mem.tick(2);
        assert!(mem.raw_nmi_pending());
        assert!(!Bus::nmi_pending(&mut mem));
    }

    #[test]
    fn cia2_icr_read_reports_the_nmi_line_was_lowered() {
        let mut mem = memory();
        mem.set_irq_routed(true);
        mem.set_byte(0xDD04, 1);
        mem.set_byte(0xDD05, 0);
        mem.set_byte(0xDD0E, 0x11); // force-load latch and start Timer A
        mem.set_byte(0xDD0D, 0x81); // enable the Timer A NMI mask
        mem.tick(2);
        assert!(mem.raw_nmi_pending());
        assert!(!mem.take_nmi_ack_seen());

        // The handler acks CIA2 by reading ICR; the player re-arms its edge
        // detector from this flag so a same-tick underflow is not lost.
        mem.get_byte(0xDD0D);
        assert!(mem.take_nmi_ack_seen());
        assert!(!mem.take_nmi_ack_seen());
    }

    #[test]
    fn vic_and_cia1_both_feed_irq_pending() {
        let mut mem = memory();
        mem.set_irq_routed(true);
        assert!(!mem.raw_irq_pending());

        // CIA1 Timer A underflow raises its IRQ line.
        mem.set_byte(0xDC04, 1);
        mem.set_byte(0xDC05, 0);
        mem.set_byte(0xDC0E, 0x11); // force-load latch and start Timer A
        mem.set_byte(0xDC0D, 0x81); // enable the Timer A IRQ mask
        mem.tick(2);
        assert!(mem.cia1.irq_line());
        assert!(mem.raw_irq_pending());

        // Reading ICR clears the pending flag, as a real handler does.
        mem.get_byte(0xDC0D);
        assert!(!mem.cia1.irq_line());
        assert!(!mem.raw_irq_pending());

        mem.set_byte(0xD012, 1);
        mem.set_byte(0xD01A, 0x01);
        mem.tick(64);
        assert!(mem.vic.irq_line());
        assert!(mem.raw_irq_pending());
    }

    #[test]
    fn kernal_irq_exits_acknowledge_cia_and_restore_registers() {
        let mut mem = memory();
        assert_eq!(mem.get_byte(0xEA31), OPCODE_LDA_ABS);
        assert_eq!(mem.get_byte(0xEA32), CIA1_ICR_LO);
        assert_eq!(mem.get_byte(0xEA33), CIA1_ICR_HI);
        for (offset, opcode) in [
            (0xEA34, OPCODE_PLA),
            (0xEA35, OPCODE_TAY),
            (0xEA36, OPCODE_PLA),
            (0xEA37, OPCODE_TAX),
            (0xEA38, OPCODE_PLA),
            (0xEA39, OPCODE_RTI),
        ] {
            assert_eq!(
                mem.get_byte(offset),
                opcode,
                "bad KERNAL exit byte at ${offset:04X}"
            );
        }

        assert_eq!(mem.get_byte(0xEA7E), OPCODE_LDA_ABS);
        assert_eq!(mem.get_byte(0xEA7F), CIA1_ICR_LO);
        assert_eq!(mem.get_byte(0xEA80), CIA1_ICR_HI);
        for (offset, opcode) in [
            (0xEA81, OPCODE_PLA),
            (0xEA82, OPCODE_TAY),
            (0xEA83, OPCODE_PLA),
            (0xEA84, OPCODE_TAX),
            (0xEA85, OPCODE_PLA),
            (0xEA86, OPCODE_RTI),
        ] {
            assert_eq!(
                mem.get_byte(offset),
                opcode,
                "bad KERNAL exit byte at ${offset:04X}"
            );
        }
    }

    #[test]
    fn kernal_irq_entry_saves_registers_and_nmi_entry_does_not() {
        // $FF48 saves A/X/Y so handlers can finish with the standard
        // PLA/TAY/PLA/TAX/PLA/RTI sequence; $FE43 only disables IRQs and jumps
        // through $0318, because NMI handlers save whatever they need themselves.
        let mut mem = memory();
        for (offset, opcode) in [
            (0xFF48, OPCODE_PHA),
            (0xFF49, OPCODE_TXA),
            (0xFF4A, OPCODE_PHA),
            (0xFF4B, OPCODE_TYA),
            (0xFF4C, OPCODE_PHA),
            (0xFF4D, OPCODE_JMP_INDIRECT),
        ] {
            assert_eq!(
                mem.get_byte(offset),
                opcode,
                "bad KERNAL entry byte at ${offset:04X}"
            );
        }
        assert_eq!(mem.get_byte(0xFF4E), 0x14, "$FF48 must JMP ($0314)");
        assert_eq!(mem.get_byte(0xFF4F), 0x03);

        assert_eq!(mem.get_byte(0xFE43), OPCODE_SEI);
        assert_eq!(mem.get_byte(0xFE44), OPCODE_JMP_INDIRECT);
        assert_eq!(mem.get_byte(0xFE45), 0x18);
        assert_eq!(mem.get_byte(0xFE46), 0x03);
    }
}
