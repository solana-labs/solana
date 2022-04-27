use serde::Serialize;

/// Gcov JSON intermediate format.
///
/// Documented in [man gcov.1](https://man7.org/linux/man-pages/man1/gcov.1.html)
#[derive(Serialize)]
pub struct GcovIntermediate {
    pub files: Vec<GcovFile>,
}

#[derive(Serialize)]
pub struct GcovFile {
    pub file: String,
    pub lines: Vec<GcovLine>,
}

#[derive(Serialize)]
pub struct GcovLine {
    pub line_number: u64,
    pub count: usize,
}
