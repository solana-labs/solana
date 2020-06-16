use thiserror::Error;

#[derive(PartialEq, Debug, Error, Eq, Clone)]
pub enum SanitizeError {
    #[error("index out of bounds")]
    IndexOutOfBounds,
    #[error("value out of bounds")]
    ValueOutOfBounds,
    #[error("invalid value")]
    InvalidValue,
}

/// Trait for sanitizing values and members of over the wire messages.
/// Implementation should recursively decent through the data structure
/// and sanitize all struct members and enum clauses.  Sanitize excludes
/// signature verification checks, those are handled by another pass.
/// Sanitize checks should include but are not limited too:
///   * All index values are in range
///   * All values are within their static max/min bounds
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
