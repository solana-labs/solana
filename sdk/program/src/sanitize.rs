//! A trait for sanitizing values and members of over the wire messages.

use {crate::instruction::InstructionError, thiserror::Error};

#[derive(PartialEq, Debug, Error, Eq, Clone)]
pub enum SanitizeError {
    #[error("index out of bounds")]
    IndexOutOfBounds,
    #[error("value out of bounds")]
    ValueOutOfBounds,
    #[error("invalid value")]
    InvalidValue,

    /// An error occurred while processing an instruction. The first element of the tuple
    /// indicates the instruction index in which the error occurred.
    #[error("Error processing Instruction {0}: {1}")]
    InstructionError(u8, InstructionError),

    /// Transaction contains a duplicate instruction that is not allowed
    #[error("Transaction contains a duplicate instruction ({0}) that is not allowed")]
    DuplicateInstruction(u8),
}

/// A trait for sanitizing values and members of over-the-wire messages.
///
/// Implementation should recursively descend through the data structure and
/// sanitize all struct members and enum clauses. Sanitize excludes signature-
/// verification checks, those are handled by another pass. Sanitize checks
/// should include but are not limited to:
///
/// - All index values are in range.
/// - All values are within their static max/min bounds.
pub trait Sanitize {
    fn sanitize(&self) -> Result<(), SanitizeError> {
        Ok(())
    }
}

impl<T: Sanitize> Sanitize for Vec<T> {
    fn sanitize(&self) -> Result<(), SanitizeError> {
        for x in self.iter() {
            x.sanitize()?;
        }
        Ok(())
    }
}
