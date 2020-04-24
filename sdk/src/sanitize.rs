pub enum SanitizeError {
    Failed,
}

pub trait Sanitize {
    fn sanitize(&self) -> Result<(), SanitizeError>;
}

impl<T: Sanitize> Sanitize for Vec<T> {
    fn sanitize(&self) -> Result<(), SanitizeError> {
        for x in self.iter() {
            x.sanitize()?;
        }
        Ok(())
    }
}
