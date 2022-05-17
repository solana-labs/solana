use std::str::FromStr;

pub const TAR_BZIP2_EXTENSION: &str = "tar.bz2";
pub const TAR_GZIP_EXTENSION: &str = "tar.gz";
pub const TAR_ZSTD_EXTENSION: &str = "tar.zst";
pub const TAR_EXTENSION: &str = "tar";

/// The different archive formats used for snapshots
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ArchiveFormat {
    TarBzip2,
    TarGzip,
    TarZstd,
    Tar,
}

impl ArchiveFormat {
    /// Get the file extension for the ArchiveFormat
    pub fn extension(&self) -> &str {
        match self {
            ArchiveFormat::TarBzip2 => TAR_BZIP2_EXTENSION,
            ArchiveFormat::TarGzip => TAR_GZIP_EXTENSION,
            ArchiveFormat::TarZstd => TAR_ZSTD_EXTENSION,
            ArchiveFormat::Tar => TAR_EXTENSION,
        }
    }
}

// Change this to `impl<S: AsRef<str>> TryFrom<S> for ArchiveFormat [...]`
// once this Rust bug is fixed: https://github.com/rust-lang/rust/issues/50133
impl TryFrom<&str> for ArchiveFormat {
    type Error = ParseError;

    fn try_from(extension: &str) -> Result<Self, Self::Error> {
        match extension {
            TAR_BZIP2_EXTENSION => Ok(ArchiveFormat::TarBzip2),
            TAR_GZIP_EXTENSION => Ok(ArchiveFormat::TarGzip),
            TAR_ZSTD_EXTENSION => Ok(ArchiveFormat::TarZstd),
            TAR_EXTENSION => Ok(ArchiveFormat::Tar),
            _ => Err(ParseError::InvalidExtension),
        }
    }
}

impl FromStr for ArchiveFormat {
    type Err = ParseError;

    fn from_str(extension: &str) -> Result<Self, Self::Err> {
        Self::try_from(extension)
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum ParseError {
    InvalidExtension,
}

#[cfg(test)]
mod tests {
    use super::*;
    const INVALID_EXTENSION: &str = "zip";

    #[test]
    fn test_extension() {
        assert_eq!(ArchiveFormat::TarBzip2.extension(), TAR_BZIP2_EXTENSION);
        assert_eq!(ArchiveFormat::TarGzip.extension(), TAR_GZIP_EXTENSION);
        assert_eq!(ArchiveFormat::TarZstd.extension(), TAR_ZSTD_EXTENSION);
        assert_eq!(ArchiveFormat::Tar.extension(), TAR_EXTENSION);
    }

    #[test]
    fn test_try_from() {
        assert_eq!(
            ArchiveFormat::try_from(TAR_BZIP2_EXTENSION),
            Ok(ArchiveFormat::TarBzip2)
        );
        assert_eq!(
            ArchiveFormat::try_from(TAR_GZIP_EXTENSION),
            Ok(ArchiveFormat::TarGzip)
        );
        assert_eq!(
            ArchiveFormat::try_from(TAR_ZSTD_EXTENSION),
            Ok(ArchiveFormat::TarZstd)
        );
        assert_eq!(
            ArchiveFormat::try_from(TAR_EXTENSION),
            Ok(ArchiveFormat::Tar)
        );
        assert_eq!(
            ArchiveFormat::try_from(INVALID_EXTENSION),
            Err(ParseError::InvalidExtension)
        );
    }

    #[test]
    fn test_from_str() {
        assert_eq!(
            ArchiveFormat::from_str(TAR_BZIP2_EXTENSION),
            Ok(ArchiveFormat::TarBzip2)
        );
        assert_eq!(
            ArchiveFormat::from_str(TAR_GZIP_EXTENSION),
            Ok(ArchiveFormat::TarGzip)
        );
        assert_eq!(
            ArchiveFormat::from_str(TAR_ZSTD_EXTENSION),
            Ok(ArchiveFormat::TarZstd)
        );
        assert_eq!(
            ArchiveFormat::from_str(TAR_EXTENSION),
            Ok(ArchiveFormat::Tar)
        );
        assert_eq!(
            ArchiveFormat::from_str(INVALID_EXTENSION),
            Err(ParseError::InvalidExtension)
        );
    }
}
