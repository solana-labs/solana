use {
    std::{fmt, str::FromStr},
    strum::Display,
};

pub const SUPPORTED_ARCHIVE_COMPRESSION: &[&str] = &["bz2", "gzip", "zstd", "lz4", "tar", "none"];

pub const TAR_BZIP2_EXTENSION: &str = "tar.bz2";
pub const TAR_GZIP_EXTENSION: &str = "tar.gz";
pub const TAR_ZSTD_EXTENSION: &str = "tar.zst";
pub const TAR_LZ4_EXTENSION: &str = "tar.lz4";
pub const TAR_EXTENSION: &str = "tar";

/// The different archive formats used for snapshots
#[derive(Copy, Clone, Debug, Eq, PartialEq, Display)]
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
            _ => Err(ParseError::InvalidExtension(extension.to_string())),
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
pub enum ParseError {
    InvalidExtension(String),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ParseError::InvalidExtension(extension) => {
                write!(f, "Invalid archive extension: {}", extension)
            }
        }
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
            Err(ParseError::InvalidExtension(INVALID_EXTENSION.to_string()))
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
            Err(ParseError::InvalidExtension(INVALID_EXTENSION.to_string()))
        );
    }

    #[test]
    fn test_to_string() {
        assert_eq!(
            ArchiveFormat::from_str(TAR_BZIP2_EXTENSION)
                .unwrap()
                .to_string(),
            "TarBzip2"
        );
        assert_eq!(
            ArchiveFormat::from_str(TAR_GZIP_EXTENSION)
                .unwrap()
                .to_string(),
            "TarGzip",
        );
        assert_eq!(
            ArchiveFormat::from_str(TAR_ZSTD_EXTENSION)
                .unwrap()
                .to_string(),
            "TarZstd"
        );
        assert_eq!(
            ArchiveFormat::from_str(TAR_LZ4_EXTENSION)
                .unwrap()
                .to_string(),
            "TarLz4",
        );
        assert_eq!(
            ArchiveFormat::from_str(TAR_EXTENSION).unwrap().to_string(),
            "Tar"
        );
    }
}
