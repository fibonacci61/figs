mod bus;
mod cartridge;
mod cpu;
mod dma;
mod joypad;
mod ppu;
mod serial;
mod timer;

use std::{
    cell::RefCell,
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant},
};

use clap::Parser;
use log::info;
use minifb::WindowOptions;

use crate::{
    bus::Bus,
    cartridge::Cartridge,
    cpu::{Cpu, IrqHolder},
    joypad::Joypad,
    ppu::{Ppu, SCREEN_HEIGHT, SCREEN_WIDTH},
    serial::Serial,
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

const GB_CLOCK: u32 = 4_194_304;
const SYNC_FREQUENCY: u32 = 4096;

fn main() -> anyhow::Result<()> {
    env_logger::init();

    let figs = Figs::parse();
    let rom = std::fs::read(&figs.rom_path)?;
    let cart = Cartridge::new(rom)?;

    info!("loaded ROM '{}'", cart.title());

    let mut window = minifb::Window::new(
        "FIGS",
        SCREEN_WIDTH,
        SCREEN_HEIGHT,
        WindowOptions {
            scale: minifb::Scale::X4,
            ..Default::default()
        },
    )
    .unwrap();

    let irq_holder = Rc::new(RefCell::new(IrqHolder::new()));
    let timer = Timer::new(Rc::clone(&irq_holder));
    let serial = Serial::new(Rc::clone(&irq_holder));
    let ppu = Ppu::new(&mut window, Rc::clone(&irq_holder));
    let joypad = Joypad::new(Rc::clone(&irq_holder));
    let bus = Bus::new(
        cart,
        ppu,
        irq_holder,
        timer,
        serial,
        figs.gameboy_doctor,
        window,
        joypad,
    );
    let mut cpu = Cpu::new(bus, figs.gameboy_doctor);

    let start = Instant::now();
    let mut total_cycles = 0;
    let mut next_sync = SYNC_FREQUENCY;
    loop {
        let machine_cycles = cpu.step();
        let cycles = machine_cycles * 4;
        total_cycles += cycles;

        for _ in 0..cycles {
            cpu.step_timer();
            cpu.step_serial();
        }

        for _ in 0..cycles {
            cpu.step_ppu();
        }

        cpu.step_dma(cycles);
        cpu.update_joypad();

        if total_cycles >= next_sync {
            next_sync += SYNC_FREQUENCY;

            let emulated_time = Duration::from_secs_f64(total_cycles as f64 / GB_CLOCK as f64);
            let target_time = start + emulated_time;
            if let Some(remaining) = target_time.checked_duration_since(Instant::now()) {
                std::thread::sleep(remaining);
            }
        }
    }
}
