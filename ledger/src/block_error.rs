#[derive(Debug, PartialEq)]
pub enum BlockError {
    /// Block entries hashes must all be valid
    InvalidEntryHash,

    /// Blocks can not have extra ticks or missing ticks
    InvalidTickCount,

    /// All ticks must contain the same number of hashes within a block
    InvalidTickHashCount,

    /// Blocks must end in a tick entry, trailing transaction entries are not allowed to guarantee
    /// that each block has the same number of hashes
    TrailingEntry,
}
