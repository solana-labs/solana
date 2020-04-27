#[derive(PartialEq, Debug)]
pub enum SanitizeError {
    Failed,
    IndexOutOfBounds,
    ValueOutOfRange,
}

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
