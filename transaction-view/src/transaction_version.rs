/// A byte that represents the version of the transaction.
#[derive(Copy, Clone, Debug, Default)]
#[repr(u8)]
pub enum TransactionVersion {
    #[default]
    Legacy = u8::MAX,
    V0 = 0,
}
