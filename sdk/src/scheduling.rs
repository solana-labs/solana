//! Primitive types relevant to transaction scheduling
#![cfg(feature = "full")]

#[derive(Debug, Clone, Copy)]
pub enum SchedulingMode {
    BlockVerification,
}

pub trait WithSchedulingMode {
    fn mode(&self) -> SchedulingMode;
}

