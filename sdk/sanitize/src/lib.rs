//! A trait for sanitizing values and members of over the wire messages.

use {core::fmt, std::error::Error};

#[derive(PartialEq, Debug, Eq, Clone)]
pub enum SanitizeError {
    IndexOutOfBounds,
    ValueOutOfBounds,
    InvalidValue,
}

impl Error for SanitizeError {}

impl fmt::Display for SanitizeError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            SanitizeError::IndexOutOfBounds => f.write_str("index out of bounds"),
            SanitizeError::ValueOutOfBounds => f.write_str("value out of bounds"),
            SanitizeError::InvalidValue => f.write_str("invalid value"),
        }
    }
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
