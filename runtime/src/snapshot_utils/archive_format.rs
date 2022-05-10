use std::{fmt, str::FromStr};

pub const TAR_BZIP2_EXTENSION: &str = "tar.bz2";
pub const TAR_GZIP_EXTENSION: &str = "tar.gz";
pub const TAR_ZSTD_EXTENSION: &str = "tar.zst";
pub const TAR_LZ4_EXTENSION: &str = "tar.lz4";
pub const TAR_EXTENSION: &str = "tar";

/// The different archive formats used for snapshots
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ArchiveFormat {
    TarBzip2,
    TarGzip,
    TarZstd,
    TarLz4,
    Tar,
}

impl ArchiveFormat {
    /// Get the file extension for the ArchiveFormat
    pub fn extension(&self) -> &str {
        match self {
            ArchiveFormat::TarBzip2 => TAR_BZIP2_EXTENSION,
            ArchiveFormat::TarGzip => TAR_GZIP_EXTENSION,
            ArchiveFormat::TarZstd => TAR_ZSTD_EXTENSION,
            ArchiveFormat::TarLz4 => TAR_LZ4_EXTENSION,
            ArchiveFormat::Tar => TAR_EXTENSION,
        }
    }
}

/// Converts to a String
impl ToString for ArchiveFormat {
    fn to_string(&self) -> String {
        match self {
            ArchiveFormat::TarBzip2 => "BZIP2".to_string(),
            ArchiveFormat::TarGzip => "GZIP".to_string(),
            ArchiveFormat::TarZstd => "ZSTD".to_string(),
            ArchiveFormat::TarLz4 => "LZ4".to_string(),
            ArchiveFormat::Tar => "TAR".to_string(),
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
            TAR_LZ4_EXTENSION => Ok(ArchiveFormat::TarLz4),
            TAR_EXTENSION => Ok(ArchiveFormat::Tar),
            _ => Err(ParseError(extension.to_string())),
        }
    }
}

impl FromStr for ArchiveFormat {
    type Err = ParseError;

    fn from_str(extension: &str) -> Result<Self, Self::Err> {
        Self::try_from(extension)
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ParseError(String);

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Invalid archive extension: {}", self.0)
    }
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
        assert_eq!(ArchiveFormat::TarLz4.extension(), TAR_LZ4_EXTENSION);
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
            ArchiveFormat::try_from(TAR_LZ4_EXTENSION),
            Ok(ArchiveFormat::TarLz4)
        );
        assert_eq!(
            ArchiveFormat::try_from(TAR_EXTENSION),
            Ok(ArchiveFormat::Tar)
        );
        assert_eq!(
            ArchiveFormat::try_from(INVALID_EXTENSION),
            Err(ParseError(INVALID_EXTENSION.to_string()))
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
            ArchiveFormat::from_str(TAR_LZ4_EXTENSION),
            Ok(ArchiveFormat::TarLz4)
        );
        assert_eq!(
            ArchiveFormat::from_str(TAR_EXTENSION),
            Ok(ArchiveFormat::Tar)
        );
        assert_eq!(
            ArchiveFormat::from_str(INVALID_EXTENSION),
            Err(ParseError(INVALID_EXTENSION.to_string()))
        );
    }
}
