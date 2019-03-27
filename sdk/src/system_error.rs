#[derive(Serialize, Debug, Clone, PartialEq)]
pub enum SystemError {
    AccountAlreadyInUse,
    ResultWithNegativeLamports,
    SourceNotSystemAccount,
}
