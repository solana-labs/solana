#[derive(Debug, Clone, Copy)]
pub enum SchedulingMode {
    BlockVerification,
}

pub trait WithSchedulingMode {
    fn mode(&self) -> SchedulingMode;
}

// This file will be populated with actual implementation later.
