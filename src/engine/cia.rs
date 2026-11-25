// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Mikael Lund

//! CIA1 (MOS 6526) emulation, scoped to what RSID files need.
//!
//! Real C64 CIAs have two timers, a 24-hour TOD clock, a serial port, and two
//! 8-bit I/O ports for keyboard / joysticks. We implement only Timer A and the
//! Interrupt Control Register — the rest of the chip is stubbed (writes
//! accepted; reads return `$FF` floating-bus). This is enough to drive the
//! ~80% of HVSC's RSID corpus that uses CIA1 Timer A IRQs for playback.

/// Register offsets within the CIA1 register file (mirrored every 16 bytes
/// across `$DC00..=$DCFF`).
const REG_TIMER_A_LO: u8 = 0x04;
const REG_TIMER_A_HI: u8 = 0x05;
const REG_TIMER_B_LO: u8 = 0x06;
const REG_TIMER_B_HI: u8 = 0x07;
const REG_ICR: u8 = 0x0D;
const REG_CRA: u8 = 0x0E;
const REG_CRB: u8 = 0x0F;
/// CIA register file is 16 bytes; addresses above mirror.
const REG_MASK: u8 = 0x0F;

/// Control Register A/B bits.
pub(crate) const CRA_START: u8 = 1 << 0;
const CRA_MODE_ONESHOT: u8 = 1 << 3;
/// Force-load is a write-only strobe: copies latch into counter then clears itself.
pub(crate) const CRA_FORCE_LOAD: u8 = 1 << 4;
/// CRB bits 5-6 = 10 (`0x40`): Timer B counts Timer A underflows instead of CPU cycles.
const CRB_INMODE_MASK: u8 = 0x60;
const CRB_INMODE_TA_UNDERFLOW: u8 = 0x40;

/// Interrupt Control Register source bits.
pub(crate) const ICR_TIMER_A: u8 = 1 << 0;
pub(crate) const ICR_TIMER_B: u8 = 1 << 1;
/// Set in the read value when any masked source is pending; mirrors the IRQ line.
const ICR_ANY: u8 = 1 << 7;
/// Bit 7 in writes selects set vs clear; bits 0-4 select which sources.
pub(crate) const ICR_FILL_BIT: u8 = 1 << 7;
const ICR_SOURCE_MASK: u8 = 0x1F;

/// Reads of unimplemented registers return floating-bus high.
const FLOATING_BUS: u8 = 0xFF;

/// Boot state of CIA1 Timer A on a real C64: KERNAL programs it for ~60Hz
/// (`$4025` = 16421 cycles). Many PSID-CIA tunes assume this is already running
/// and only adjust the latch — without re-issuing the start/force-load strobe —
/// so we must come out of reset already armed. The exact value matters less
/// than being non-zero and giving roughly VBI-rate firing for tunes that don't
/// reprogram the CIA at all.
const KERNAL_DEFAULT_LATCH: u16 = 0x4025;

/// MOS 6526 CIA — Timer A + Timer B + Interrupt Control Register.
/// Used for both CIA1 (IRQ) and CIA2 (NMI).
pub struct Cia {
    /// Decrementing 16-bit Timer A counter (per CPU cycle when `CRA_START` is set).
    counter_a: u16,
    /// Reloaded into `counter_a` on underflow (continuous) or force-load.
    latch_a: u16,
    /// Control register A; force-load strobe is acted on but not stored.
    cra: u8,
    /// Decrementing 16-bit Timer B counter.
    counter_b: u16,
    /// Reloaded into `counter_b` on underflow (continuous) or force-load.
    latch_b: u16,
    /// Control register B; force-load strobe is acted on but not stored.
    crb: u8,
    /// Pending interrupt sources, bits 0-4.
    icr_pending: u8,
    /// Mask: bits 0-4 enable per-source IRQ/NMI assertion.
    icr_mask: u8,
}

pub type Cia1 = Cia;

impl Default for Cia {
    fn default() -> Self {
        // Boot already armed — see KERNAL_DEFAULT_LATCH note above.
        Self {
            counter_a: KERNAL_DEFAULT_LATCH,
            latch_a: KERNAL_DEFAULT_LATCH,
            cra: CRA_START,
            counter_b: 0xFFFF,
            latch_b: 0xFFFF,
            crb: 0,
            icr_pending: 0,
            icr_mask: 0,
        }
    }
}

impl Cia {
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a CIA with Timer A stopped (suitable for CIA2 at boot).
    pub fn new_stopped() -> Self {
        Self {
            cra: 0,
            ..Self::default()
        }
    }

    /// Bus read of CIA register `reg` (low 4 bits significant; mirrors).
    pub fn read_register(&mut self, reg: u8) -> u8 {
        #[allow(clippy::cast_possible_truncation)]
        match reg & REG_MASK {
            REG_TIMER_A_LO => (self.counter_a & 0x00FF) as u8,
            REG_TIMER_A_HI => (self.counter_a >> 8) as u8,
            REG_TIMER_B_LO => (self.counter_b & 0x00FF) as u8,
            REG_TIMER_B_HI => (self.counter_b >> 8) as u8,
            REG_ICR => {
                // Returning ICR atomically reads pending+IRQ line and clears both.
                let any = if self.irq_line() { ICR_ANY } else { 0 };
                let value = (self.icr_pending & ICR_SOURCE_MASK) | any;
                self.icr_pending = 0;
                value
            }
            REG_CRA => self.cra & !CRA_FORCE_LOAD,
            REG_CRB => self.crb & !CRA_FORCE_LOAD,
            _ => FLOATING_BUS,
        }
    }

    /// Bus write of CIA register `reg`.
    pub fn write_register(&mut self, reg: u8, val: u8) {
        match reg & REG_MASK {
            REG_TIMER_A_LO => self.latch_a = (self.latch_a & 0xFF00) | u16::from(val),
            REG_TIMER_A_HI => self.latch_a = (self.latch_a & 0x00FF) | (u16::from(val) << 8),
            REG_TIMER_B_LO => self.latch_b = (self.latch_b & 0xFF00) | u16::from(val),
            REG_TIMER_B_HI => self.latch_b = (self.latch_b & 0x00FF) | (u16::from(val) << 8),
            REG_ICR => {
                // Bit 7 = 1 → set the source bits in the mask; bit 7 = 0 → clear them.
                let sources = val & ICR_SOURCE_MASK;
                if val & ICR_FILL_BIT != 0 {
                    self.icr_mask |= sources;
                } else {
                    self.icr_mask &= !sources;
                }
            }
            REG_CRA => {
                if val & CRA_FORCE_LOAD != 0 {
                    self.counter_a = self.latch_a;
                }
                self.cra = val & !CRA_FORCE_LOAD;
            }
            REG_CRB => {
                if val & CRA_FORCE_LOAD != 0 {
                    self.counter_b = self.latch_b;
                }
                self.crb = val & !CRA_FORCE_LOAD;
            }
            _ => {}
        }
    }

    /// Advance Timer A and Timer B by `cycles`.
    pub fn tick(&mut self, cycles: u64) {
        let ta_underflows = tick_timer(
            &mut self.counter_a,
            self.latch_a,
            &mut self.cra,
            cycles,
            &mut self.icr_pending,
            ICR_TIMER_A,
        );

        let tb_ticks = if (self.crb & CRB_INMODE_MASK) == CRB_INMODE_TA_UNDERFLOW {
            ta_underflows
        } else {
            cycles
        };
        if tb_ticks > 0 {
            tick_timer(
                &mut self.counter_b,
                self.latch_b,
                &mut self.crb,
                tb_ticks,
                &mut self.icr_pending,
                ICR_TIMER_B,
            );
        }
    }

    /// True when an enabled interrupt source is pending — drives `Bus::irq_pending`.
    pub fn irq_line(&self) -> bool {
        (self.icr_pending & self.icr_mask & ICR_SOURCE_MASK) != 0
    }

    /// Polls + clears the Timer A underflow flag without disturbing the mask.
    /// Used by PSID-CIA mode where the player drives `play()` directly instead
    /// of relying on the CPU to service an IRQ.
    pub fn take_timer_a_underflow(&mut self) -> bool {
        let fired = self.icr_pending & ICR_TIMER_A != 0;
        self.icr_pending &= !ICR_TIMER_A;
        fired
    }
}

/// Advances one CIA timer by `ticks`, returning the number of underflows.
fn tick_timer(
    counter: &mut u16,
    latch: u16,
    cr: &mut u8,
    ticks: u64,
    icr_pending: &mut u8,
    icr_bit: u8,
) -> u64 {
    if *cr & CRA_START == 0 {
        return 0;
    }
    if latch == 0 {
        *icr_pending |= icr_bit;
        return 1;
    }
    let mut remaining = ticks;
    let mut underflows = 0u64;
    while remaining > 0 {
        let to_underflow = u64::from(*counter) + 1;
        if remaining < to_underflow {
            #[allow(clippy::cast_possible_truncation)]
            {
                *counter -= remaining as u16;
            }
            return underflows;
        }
        remaining -= to_underflow;
        underflows += 1;
        *icr_pending |= icr_bit;
        if *cr & CRA_MODE_ONESHOT != 0 {
            *cr &= !CRA_START;
            *counter = latch;
            return underflows;
        }
        *counter = latch;
    }
    underflows
}
