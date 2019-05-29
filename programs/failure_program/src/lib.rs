use solana_sdk::account::KeyedAccount;
use solana_sdk::credit_only_account::KeyedCreditOnlyAccount;
use solana_sdk::instruction::InstructionError;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::solana_entrypoint;

solana_entrypoint!(entrypoint);
fn entrypoint(
    _program_id: &Pubkey,
    _keyed_accounts: &mut [KeyedAccount],
    _keyed_credit_only_accounts: &mut [KeyedCreditOnlyAccount],
    _data: &[u8],
    _tick_height: u64,
) -> Result<(), InstructionError> {
    Err(InstructionError::GenericError)
}
