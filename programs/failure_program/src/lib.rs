use solana_sdk::account_api::AccountWrapper;
use solana_sdk::instruction::InstructionError;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::solana_entrypoint;

solana_entrypoint!(entrypoint);
fn entrypoint(
    _program_id: &Pubkey,
    _keyed_accounts: &mut [AccountWrapper],
    _data: &[u8],
) -> Result<(), InstructionError> {
    Err(InstructionError::GenericError)
}
