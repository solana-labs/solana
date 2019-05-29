use crate::token_state::TokenState;
use log::*;
use solana_sdk::account::KeyedAccount;
use solana_sdk::credit_only_account::KeyedCreditOnlyAccount;
use solana_sdk::instruction::InstructionError;
use solana_sdk::pubkey::Pubkey;

pub fn process_instruction(
    program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    _keyed_credit_only_accounts: &mut [KeyedCreditOnlyAccount],
    input: &[u8],
    _tick_height: u64,
) -> Result<(), InstructionError> {
    solana_logger::setup();

    TokenState::process(program_id, keyed_accounts, input).map_err(|e| {
        error!("error: {:?}", e);
        InstructionError::CustomError(e as u32)
    })
}
