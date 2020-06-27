use solana_sdk_program_macros::instructions;

mod test_program;

#[instructions(test_program::id())]
#[derive(Clone, Debug, PartialEq)]
pub enum TestInstruction {
    #[accounts(
        from_account(SIGNER, WRITABLE, desc = "Funding account")
    )]
    Transfer { lamports: u64 },
}

fn main() {}
