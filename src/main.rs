mod bus;
mod cartridge;
mod cpu;

use std::path::PathBuf;

use clap::Parser;

use crate::{bus::Bus, cartridge::Cartridge, cpu::Cpu};

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
    let bus = Bus::new(cart);

    let mut cpu = Cpu::new(bus);
    for _ in 0..100 {
        cpu.next();
    }

    Ok(())
}
