use std::{cell::RefCell, rc::Rc};

use crate::cpu::IrqHolder;

// In Non-CGB mode the Game Boy supplies an internal serial clock of 8192Hz. The
// CPU runs at 4194304Hz, so each transferred bit takes 4194304 / 8192 = 512
// T-cycles (and a full byte takes 8 * 512 = 4096).
const T_CYCLES_PER_BIT: u32 = 512;

#[bitfields::bitfield(u8)]
pub struct Sc {
    // 0 = external clock (the other Game Boy clocks the transfer), 1 = internal
    shift_clock: bool,
    // CGB only, ignored on DMG
    clock_speed: bool,
    #[bits(5)]
    _reserved: u8,
    transfer_start: bool,
}

struct Transfer {
    bits_remaining: u8,
    cycles: u32,
}

pub struct Serial {
    sb: u8,
    sc: Sc,
    transfer: Option<Transfer>,
    irq_holder: Rc<RefCell<IrqHolder>>,
}

impl Serial {
    pub fn new(irq_holder: Rc<RefCell<IrqHolder>>) -> Self {
        Self {
            sb: 0x00,
            sc: Sc::from_bits(0x7E),
            transfer: None,
            irq_holder,
        }
    }

    pub fn sb(&self) -> u8 {
        self.sb
    }

    pub fn set_sb(&mut self, value: u8) {
        self.sb = value;
    }

    pub fn sc(&self) -> u8 {
        // bits 1..=6 are unused and read as 1
        self.sc.into_bits() | 0x7E
    }

    pub fn set_sc(&mut self, value: u8) {
        self.sc = Sc::from_bits(value);
        if self.sc.transfer_start() && self.sc.shift_clock() {
            // We provide the clock, so the transfer proceeds on its own.
            self.transfer = Some(Transfer {
                bits_remaining: 8,
                cycles: 0,
            });
        } else if !self.sc.transfer_start() {
            self.transfer = None;
        }
        // A transfer requested with the external clock (no cable attached) never
        // receives a clock, so it stays pending forever and is left untouched.
    }

    pub fn step(&mut self) {
        let Some(transfer) = self.transfer.as_mut() else {
            return;
        };

        transfer.cycles += 1;
        if transfer.cycles < T_CYCLES_PER_BIT {
            return;
        }
        transfer.cycles = 0;
        transfer.bits_remaining -= 1;
        let done = transfer.bits_remaining == 0;

        // With no external Game Boy connected, ones are shifted in, so SB ends
        // up holding 0xFF once the transfer completes.
        self.sb = (self.sb << 1) | 1;

        if done {
            self.transfer = None;
            self.sc.set_transfer_start(false);
            self.irq_holder.borrow_mut().request_serial();
        }
    }
}
