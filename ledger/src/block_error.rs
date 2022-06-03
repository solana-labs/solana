use thiserror::Error;

#[derive(Error, Debug, PartialEq, Eq)]
pub enum BlockError {
    /// Block did not have enough ticks was not marked full
    /// and no shred with is_last was seen.
    #[error("incomplete block")]
    Incomplete,

    /// Block entries hashes must all be valid
    #[error("invalid entry hash")]
    InvalidEntryHash,

    /// Blocks must end in a tick that has been marked as the last tick.
    #[error("invalid last tick")]
    InvalidLastTick,

    /// Blocks can not have missing ticks
    /// Usually indicates that the node was interrupted with a more valuable block during
    /// production and abandoned it for that more-favorable block. Leader sent data to indicate
    /// the end of the block.
    #[error("too few ticks")]
    TooFewTicks,

    /// Blocks can not have extra ticks
    #[error("too many ticks")]
    TooManyTicks,

    /// All ticks must contain the same number of hashes within a block
    #[error("invalid tick hash count")]
    InvalidTickHashCount,

    /// Blocks must end in a tick entry, trailing transaction entries are not allowed to guarantee
    /// that each block has the same number of hashes
    #[error("trailing entry")]
    TrailingEntry,

    #[error("duplicate block")]
    DuplicateBlock,
}
