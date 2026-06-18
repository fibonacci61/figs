use std::{cell::RefCell, rc::Rc};

use crate::cpu::IrqHolder;

#[derive(Clone, Copy, PartialEq, Eq)]
struct FallingEdgeDetector(bool);

impl FallingEdgeDetector {
    fn update(&mut self, value: bool) -> bool {
        let ret = if !value && self.0 != value {
            true
        } else {
            false
        };
        self.0 = value;
        ret
    }
}

struct TimaWrite {
    countdown: u8,
}

pub struct Timer {
    tima: u8,
    tima_write: Option<TimaWrite>,
    tma: u8,
    tac: Tac,

    counter: u16,
    edge_detector: FallingEdgeDetector,
    irq_holder: Rc<RefCell<IrqHolder>>,
}

#[bitfields::bitfield(u8)]
pub struct Tac {
    #[bits(2)]
    clock_select: u8,
    enable: bool,
    #[bits(5)]
    _reserved: u8,
}

impl Tac {
    fn clock_bit(&self) -> u16 {
        match self.clock_select() {
            0 => 9,
            1 => 3,
            2 => 5,
            3 => 7,
            _ => unreachable!(),
        }
    }
}

impl Timer {
    pub fn new(irq_holder: Rc<RefCell<IrqHolder>>) -> Self {
        Self {
            tima: 0x00,
            tima_write: None,
            tma: 0x00,
            tac: Tac::from_bits(0xF8),
            counter: 0,
            edge_detector: FallingEdgeDetector(false),
            irq_holder,
        }
    }

    pub fn div(&self) -> u8 {
        (self.counter >> 8) as u8
    }

    pub fn reset_div(&mut self) {
        self.counter = 0;
        self.sync();
    }

    pub fn tima(&self) -> u8 {
        self.tima
    }

    pub fn set_tima(&mut self, value: u8) {
        if self.tima_write.is_some() {
            self.tima_write = None;
        }
        self.tima = value;
    }

    pub fn tma(&self) -> u8 {
        self.tma
    }

    pub fn set_tma(&mut self, value: u8) {
        self.tma = value;
    }

    pub fn tac(&self) -> u8 {
        self.tac.into_bits()
    }

    pub fn set_tac(&mut self, value: u8) {
        self.tac = Tac::from_bits(value | 0xF8);
        self.sync();
    }

    fn sync(&mut self) {
        if self
            .edge_detector
            .update(self.counter & (1 << self.tac.clock_bit()) != 0 && self.tac.enable())
        {
            match self.tima.checked_add(1) {
                Some(v) => self.tima = v,
                None => {
                    // TIMA overflowed: it reads as 0 until TMA is loaded in 4 T-cycles
                    self.tima = 0;
                    self.tima_write = Some(TimaWrite { countdown: 4 });
                }
            };
        }
    }

    pub fn step(&mut self) {
        if let Some(tima_write) = self.tima_write.as_mut() {
            tima_write.countdown -= 1;
            if tima_write.countdown == 0 {
                self.tima_write = None;
                // reload TIMA with TMA and fire the interrupt together
                self.tima = self.tma;
                self.irq_holder.borrow_mut().request_timer();
            }
        }

        self.counter = self.counter.wrapping_add(1);
        self.sync();
    }
}
