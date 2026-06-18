mod bus;
mod cartridge;
mod cpu;
mod dma;
mod ppu;
mod timer;

use std::{cell::RefCell, path::PathBuf, rc::Rc};

use clap::Parser;
use log::info;
use minifb::WindowOptions;

use crate::{
    bus::Bus,
    cartridge::Cartridge,
    cpu::{Cpu, IrqHolder},
    ppu::{Ppu, SCREEN_HEIGHT, SCREEN_WIDTH},
    timer::Timer,
};

#[derive(Parser)]
#[command(version, about)]
struct Figs {
    /// Prints CPU state logs and hardcodes `LY` register to 0x90 to comply with Gameboy Doctor
    #[arg(short, long)]
    gameboy_doctor: bool,
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

    let irq_holder = Rc::new(RefCell::new(IrqHolder::new()));
    let timer = Timer::new(Rc::clone(&irq_holder));
    let ppu = Ppu::new(window, Rc::clone(&irq_holder));
    let bus = Bus::new(cart, ppu, irq_holder, timer, figs.gameboy_doctor);
    let mut cpu = Cpu::new(bus, figs.gameboy_doctor);
    loop {
        let machine_cycles = cpu.step();
        let cycles = machine_cycles * 4;

        for _ in 0..cycles {
            cpu.step_timer();
        }

        for _ in 0..machine_cycles {
            cpu.step_ppu();
        }

        cpu.step_dma(cycles);
    }
}
