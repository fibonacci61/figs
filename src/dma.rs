pub const OP_DURATION: u32 = 640;
pub const DEST_BASE_ADDR: u16 = 0xFE00;
pub const PAYLOAD_SIZE: u16 = 0x9F;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct DmaOp {
    src_addr: u16,
    remaining_cycles: u32,
}

pub struct Dma {
    current_op: Option<DmaOp>,
}

#[must_use]
pub enum DmaState {
    Free,
    Working,
    Done { src_addr: u16 },
}

impl Dma {
    pub fn new() -> Self {
        Self { current_op: None }
    }

    pub fn assign_op(&mut self, src_addr: u16) {
        self.current_op = Some(DmaOp {
            src_addr,
            remaining_cycles: OP_DURATION,
        })
    }

    pub fn is_working(&self) -> bool {
        self.current_op.is_some()
    }

    pub fn step(&mut self, cycles: u32) -> DmaState {
        match self.current_op.as_mut() {
            Some(op) => {
                let (remaining, overflown) = op.remaining_cycles.overflowing_sub(cycles);
                op.remaining_cycles = remaining;

                if overflown {
                    let src_addr = op.src_addr;
                    self.current_op = None;
                    DmaState::Done { src_addr }
                } else {
                    DmaState::Working
                }
            }
            None => DmaState::Free,
        }
    }
}
