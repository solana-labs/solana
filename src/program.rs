/// Reasons a program might have rejected an instruction.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum ProgramError {
    /// Contract's transactions resulted in an account with a negative balance
    /// The difference from InsufficientFundsForFee is that the transaction was executed by the
    /// contract
    ResultWithNegativeTokens,

    /// The program returned an error
    RuntimeError,
}
