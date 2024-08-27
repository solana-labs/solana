/// Trait for accessing transaction data from an abstract byte container.
pub trait TransactionData {
    /// Returns a reference to the serialized transaction data.
    fn data(&self) -> &[u8];
}

impl TransactionData for &[u8] {
    #[inline]
    fn data(&self) -> &[u8] {
        self
    }
}
