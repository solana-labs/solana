//! Defines the [`LamportsError`] type.

use {crate::instruction::InstructionError, thiserror::Error};

#[derive(Debug, Error)]
pub enum LamportsError {
    /// arithmetic underflowed
    #[error("Arithmetic underflowed")]
    ArithmeticUnderflow,

    /// arithmetic overflowed
    #[error("Arithmetic overflowed")]
    ArithmeticOverflow,
}

impl From<LamportsError> for InstructionError {
    fn from(error: LamportsError) -> Self {
        match error {
            LamportsError::ArithmeticOverflow => InstructionError::ArithmeticOverflow,
            LamportsError::ArithmeticUnderflow => InstructionError::ArithmeticOverflow,
        }
    }
}
