mod bus;
mod cartridge;
mod cpu;
mod dma;
mod ppu;

use std::{cell::RefCell, collections::VecDeque, path::PathBuf, rc::Rc};

use clap::Parser;
use log::info;
use minifb::WindowOptions;

use crate::{
    bus::Bus,
    cartridge::Cartridge,
    cpu::Cpu,
    ppu::{Ppu, SCREEN_HEIGHT, SCREEN_WIDTH},
};

#[derive(Parser)]
#[command(version, about)]
struct Figs {
    /// Path to Game Boy ROM
    rom_path: PathBuf,
}

fn main() -> anyhow::Result<()> {
    env_logger::init();

    let figs = Figs::parse();
    let rom = std::fs::read(&figs.rom_path)?;
    let cart = Cartridge::new(rom)?;

    info!("loaded ROM '{}'", cart.title());

    let window = minifb::Window::new(
        "FIGS",
        SCREEN_WIDTH,
        SCREEN_HEIGHT,
        WindowOptions::default(),
    )
    .unwrap();

    let int_queue = Rc::new(RefCell::new(VecDeque::new()));
    let ppu = Ppu::new(window, Rc::clone(&int_queue));
    let bus = Bus::new(cart, ppu);
    let mut cpu = Cpu::new(bus, int_queue);
    for _ in 0..32000 {
        let machine_cycles = cpu.step();
        let cycles = machine_cycles * 4;

        for _ in 0..machine_cycles {
            cpu.step_ppu();
        }

        cpu.step_dma(cycles);
    }

    Ok(())
}
