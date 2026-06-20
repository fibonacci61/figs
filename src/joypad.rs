use std::{cell::RefCell, rc::Rc};

use minifb::{Key, Window};

use crate::cpu::IrqHolder;

const RIGHT_A: u8 = 0x01;
const LEFT_B: u8 = 0x02;
const UP_SELECT: u8 = 0x04;
const DOWN_START: u8 = 0x08;
const SELECT_DPAD: u8 = 0x10;
const SELECT_BUTTONS: u8 = 0x20;

pub struct Joypad {
    status: u8,
    irq_holder: Rc<RefCell<IrqHolder>>,
}

impl Joypad {
    pub fn new(irq_holder: Rc<RefCell<IrqHolder>>) -> Self {
        Self {
            status: 0xCF,
            irq_holder,
        }
    }

    pub fn update(&mut self, window: &Window) {
        // if both dpad and buttons are selected, or if neither are:
        if (self.status & SELECT_DPAD == 0) == (self.status & SELECT_BUTTONS == 0) {
            // set all inputs to 'released' and return
            self.status |= 0x0F;
            return;
        }

        let mut new_status = 0x0F;
        for key in window.get_keys().iter() {
            if self.status & SELECT_DPAD != 0 {
                match key {
                    Key::Right => new_status &= !RIGHT_A,
                    Key::Left => new_status &= !LEFT_B,
                    Key::Up => new_status &= !UP_SELECT,
                    Key::Down => new_status &= !DOWN_START,
                    _ => {}
                }
            } else {
                match key {
                    Key::A => new_status &= !RIGHT_A,
                    Key::B => new_status &= !LEFT_B,
                    Key::Z => new_status &= !UP_SELECT,
                    Key::X => new_status &= !DOWN_START,
                    _ => {}
                }
            }
        }

        // check for any input bits that have gone from 1 ('released') to 0 ('pressed')
        let mut falling_edge = false;
        for bit in 0..4 {
            if self.status & (1 << bit) != 0 && new_status & (1 << bit) == 0 {
                falling_edge = true;
                break;
            }
        }

        if falling_edge {
            self.irq_holder.borrow_mut().request_joypad();
        }

        self.status = new_status;
    }

    pub fn status(&self) -> u8 {
        self.status
    }

    pub fn set_status(&mut self, value: u8) {
        self.status = value;
    }
}
