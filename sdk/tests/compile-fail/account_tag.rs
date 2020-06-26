use serde_derive::{Deserialize, Serialize};
use solana_sdk_program_macros::instructions;

mod test_program;

#[instructions(test_program::id())]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum TestInstruction {
    #[accounts(
        from_account(writable, desc = "Account"),
    )]
    Transfer { lamports: u64 },
}

fn main() {}
