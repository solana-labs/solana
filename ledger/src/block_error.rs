use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum BlockError {
    /// Block entries hashes must all be valid
    #[error("invalid entry hash")]
    InvalidEntryHash,

    /// Blocks must end in a tick that has been marked as the last tick.
    #[error("invalid last tick")]
    InvalidLastTick,

    /// Blocks can not have extra ticks or missing ticks
    #[error("invalid tick count")]
    InvalidTickCount,

    /// All ticks must contain the same number of hashes within a block
    #[error("invalid tick hash count")]
    InvalidTickHashCount,

    /// Blocks must end in a tick entry, trailing transaction entries are not allowed to guarantee
    /// that each block has the same number of hashes
    #[error("trailing entry")]
    TrailingEntry,
}
