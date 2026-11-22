// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2026 Emarti

//! Minimal VIC-II (MOS 6567/6569) emulation for SID music playback.
//!
//! Emulates raster counter, raster compare, and VIC-II raster IRQs ($D011, $D012, $D019, $D01A).
//! Also accepts writes to colors and control registers ($D000..=$D03F) used by tunes for raster
//! timing or border flashing.

const REG_CR1: u8 = 0x11;
const REG_RASTER: u8 = 0x12;
const REG_IRR: u8 = 0x19;
const REG_IMR: u8 = 0x1A;

const REG_MASK: u8 = 0x3F;

/// PAL: 312 raster lines per frame, 19656 cycles per frame (63 cycles per line).
pub const PAL_LINES_PER_FRAME: u16 = 312;
pub const PAL_CYCLES_PER_FRAME: u32 = 19_656;

/// NTSC: 263 raster lines per frame, 17045 cycles per frame (~64.8 cycles per line).
pub const NTSC_LINES_PER_FRAME: u16 = 263;
pub const NTSC_CYCLES_PER_FRAME: u32 = 17_045;

/// Minimal VIC-II controller.
pub struct Vic {
    regs: [u8; 64],
    cycle_in_frame: u32,
    cycles_per_frame: u32,
    lines_per_frame: u16,
    raster_compare: u16,
    irq_flag: u8,
    irq_mask: u8,
}

impl Default for Vic {
    fn default() -> Self {
        Self::new(true)
    }
}

impl Vic {
    /// Creates a VIC-II configured for PAL (true) or NTSC (false).
    pub fn new(is_pal: bool) -> Self {
        let (cycles_per_frame, lines_per_frame) = if is_pal {
            (PAL_CYCLES_PER_FRAME, PAL_LINES_PER_FRAME)
        } else {
            (NTSC_CYCLES_PER_FRAME, NTSC_LINES_PER_FRAME)
        };
        let mut regs = [0u8; 64];
        regs[REG_CR1 as usize] = 0x1B; // Standard 25-row text mode on boot
        Self {
            regs,
            cycle_in_frame: 0,
            cycles_per_frame,
            lines_per_frame,
            raster_compare: 0,
            irq_flag: 0,
            irq_mask: 0,
        }
    }

    /// Resets the raster state while keeping video standard configuration.
    pub fn reset(&mut self) {
        self.regs.fill(0);
        self.regs[REG_CR1 as usize] = 0x1B;
        self.cycle_in_frame = 0;
        self.raster_compare = 0;
        self.irq_flag = 0;
        self.irq_mask = 0;
    }

    /// Configures the video standard (PAL vs NTSC).
    pub fn set_pal(&mut self, is_pal: bool) {
        if is_pal {
            self.cycles_per_frame = PAL_CYCLES_PER_FRAME;
            self.lines_per_frame = PAL_LINES_PER_FRAME;
        } else {
            self.cycles_per_frame = NTSC_CYCLES_PER_FRAME;
            self.lines_per_frame = NTSC_LINES_PER_FRAME;
        }
        self.cycle_in_frame %= self.cycles_per_frame;
    }

    /// Current raster scanline (0..=lines_per_frame - 1).
    #[inline]
    pub fn current_line(&self) -> u16 {
        ((u64::from(self.cycle_in_frame) * u64::from(self.lines_per_frame))
            / u64::from(self.cycles_per_frame)) as u16
    }

    /// True when an enabled interrupt source (raster IRQ) is pending.
    #[inline]
    pub fn irq_line(&self) -> bool {
        (self.irq_flag & self.irq_mask & 0x0F) != 0
    }

    /// Reads a VIC-II register ($D000..$D03F).
    pub fn read_register(&self, reg: u8) -> u8 {
        match reg & REG_MASK {
            REG_CR1 => {
                let msb = if self.current_line() >= 256 {
                    0x80
                } else {
                    0x00
                };
                msb | (self.regs[REG_CR1 as usize] & 0x7F)
            }
            REG_RASTER => (self.current_line() & 0xFF) as u8,
            REG_IRR => {
                let any = if self.irq_line() { 0x80 } else { 0x00 };
                0x70 | any | (self.irq_flag & 0x0F)
            }
            REG_IMR => 0xF0 | (self.irq_mask & 0x0F),
            r => self.regs[r as usize],
        }
    }

    /// Writes a VIC-II register ($D000..$D03F).
    pub fn write_register(&mut self, reg: u8, val: u8) {
        let r = reg & REG_MASK;
        self.regs[r as usize] = val;
        // Deliberately no raster compare here: the real VIC evaluates the compare
        // once per scanline in `tick`. Re-checking it on a $D012 write re-asserted
        // an already-ACKed IRQ inside its own handler, calling play() 2-3x per
        // frame in tunes that restore the same raster line (Myth, Arkanoid).
        match r {
            REG_CR1 => {
                self.raster_compare = (self.raster_compare & 0x00FF) | (((val as u16) & 0x80) << 1);
            }
            REG_RASTER => {
                self.raster_compare = (self.raster_compare & 0x0100) | u16::from(val);
            }
            REG_IRR => {
                // Writing 1 to a bit acknowledges and clears that interrupt flag.
                // On real 6510 hardware, `INC $D019` acknowledges the raster IRQ via the
                // 6502 Read-Modify-Write dummy write cycle (which writes the unincremented
                // value with bit 0 set). Single-write 6502 emulators only perform the final write
                // ($F2), so we also clear bit 0 if bit 1 is set or if bit 0 was set.
                let mut clear_mask = val & 0x0F;
                if (val & 0x02 != 0) && (self.irq_flag & 0x01 != 0) {
                    clear_mask |= 0x01;
                }
                self.irq_flag &= !clear_mask;
            }
            REG_IMR => {
                self.irq_mask = val & 0x0F;
            }
            _ => {}
        }
    }

    /// Raster line the tune compares against, for tests and diagnostics.
    #[cfg(test)]
    pub(crate) const fn raster_compare_line(&self) -> u16 {
        self.raster_compare
    }

    /// Interrupt flag bits, for tests and diagnostics.
    #[cfg(test)]
    pub(crate) const fn irq_flag_bits(&self) -> u8 {
        self.irq_flag
    }

    /// Advances the raster beam by `elapsed` CPU cycles and asserts raster IRQ if matched.
    pub fn tick(&mut self, elapsed: u64) {
        if elapsed == 0 {
            return;
        }
        let prev_line = self.current_line();
        self.cycle_in_frame =
            ((u64::from(self.cycle_in_frame) + elapsed) % u64::from(self.cycles_per_frame)) as u32;
        let new_line = self.current_line();

        if prev_line != new_line && self.raster_compare < self.lines_per_frame {
            let matched = if prev_line < new_line {
                self.raster_compare > prev_line && self.raster_compare <= new_line
            } else {
                self.raster_compare > prev_line || self.raster_compare <= new_line
            };
            if matched {
                // The flag is independent of the mask: polling tunes read $D019
                // directly, while `irq_line` applies the mask for CPU dispatch.
                self.irq_flag |= 0x01;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cycles into a PAL frame where the given raster line starts.
    fn pal_line_start(line: u16) -> u64 {
        u64::from(line) * u64::from(PAL_CYCLES_PER_FRAME / u32::from(PAL_LINES_PER_FRAME))
    }

    #[test]
    fn raster_compare_combines_d011_and_d012() {
        let mut vic = Vic::new(true);
        vic.write_register(REG_CR1, 0x9B);
        vic.write_register(REG_RASTER, 0x34);
        assert_eq!(vic.raster_compare_line(), 0x134);

        vic.write_register(REG_CR1, 0x1B);
        assert_eq!(vic.raster_compare_line(), 0x034);
    }

    #[test]
    fn raster_irq_fires_when_compare_line_is_reached() {
        let mut vic = Vic::new(true);
        vic.write_register(REG_RASTER, 10);
        vic.write_register(REG_IMR, 0x01);
        assert!(!vic.irq_line());

        vic.tick(pal_line_start(10) + 1);
        assert_eq!(vic.irq_flag_bits() & 0x01, 0x01);
        assert!(vic.irq_line());
    }

    #[test]
    fn raster_flag_is_set_even_when_irq_is_masked() {
        let mut vic = Vic::new(true);
        vic.write_register(REG_RASTER, 10);
        vic.write_register(REG_IMR, 0x00);

        vic.tick(pal_line_start(10) + 1);
        assert_eq!(vic.irq_flag_bits() & 0x01, 0x01);
        assert!(!vic.irq_line());
    }

    #[test]
    fn writing_d019_acknowledges_raster_irq() {
        let mut vic = Vic::new(true);
        vic.write_register(REG_RASTER, 10);
        vic.write_register(REG_IMR, 0x01);
        vic.tick(pal_line_start(10) + 1);
        assert!(vic.irq_line());

        vic.write_register(REG_IRR, 0x01);
        assert_eq!(vic.irq_flag_bits() & 0x01, 0);
        assert!(!vic.irq_line());
    }

    #[test]
    fn inc_d019_acknowledges_raster_irq() {
        // `INC $D019` reads $F1 and writes $F2 on a single-write emulator.
        let mut vic = Vic::new(true);
        vic.write_register(REG_RASTER, 10);
        vic.write_register(REG_IMR, 0x01);
        vic.tick(pal_line_start(10) + 1);

        vic.write_register(REG_IRR, 0xF2);
        assert_eq!(vic.irq_flag_bits() & 0x01, 0);
    }

    #[test]
    fn compare_written_for_current_line_does_not_refire_until_next_frame() {
        let mut vic = Vic::new(true);
        vic.write_register(REG_RASTER, 1);
        vic.write_register(REG_IMR, 0x01);
        vic.tick(pal_line_start(1) + 1);
        assert!(vic.irq_line(), "first crossing still raises the IRQ");

        // A real VIC evaluates the raster compare once per line. Rewriting the
        // same compare while still on that line must not re-assert a cleared flag.
        vic.write_register(REG_IRR, 0x01);
        assert!(!vic.irq_line());
        vic.write_register(REG_RASTER, 1);
        assert!(
            !vic.irq_line(),
            "rewriting the current compare line must not restart the IRQ"
        );

        // Beam returns to line 0, then crosses line 1 again on the next frame.
        vic.tick(u64::from(PAL_CYCLES_PER_FRAME) - pal_line_start(1) - 1);
        vic.tick(pal_line_start(1) + 1);
        assert!(vic.irq_line(), "IRQ returns on the next frame");
    }

    #[test]
    fn raster_counter_wraps_after_one_pal_frame() {
        let mut vic = Vic::new(true);
        assert_eq!(vic.current_line(), 0);

        vic.tick(u64::from(PAL_CYCLES_PER_FRAME) - 1);
        assert_eq!(vic.current_line(), PAL_LINES_PER_FRAME - 1);

        vic.tick(1);
        assert_eq!(vic.current_line(), 0);
    }

    #[test]
    fn ntsc_timing_uses_ntsc_frame_length() {
        let mut vic = Vic::new(false);
        vic.tick(u64::from(NTSC_CYCLES_PER_FRAME) - 1);
        assert_eq!(vic.current_line(), NTSC_LINES_PER_FRAME - 1);

        vic.set_pal(true);
        assert!(vic.current_line() < PAL_LINES_PER_FRAME);
    }
}
