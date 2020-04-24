struct SanitizeError {
    Failed,
}

pub trait Sanitize {
    fn sanitize(&self) -> Result<SanitizeError, ()>
}
