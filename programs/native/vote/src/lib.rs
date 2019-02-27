//! Vote program
//! Receive and processes votes from validators

use bincode::deserialize;
use log::*;
use solana_sdk::account::KeyedAccount;
use solana_sdk::native_program::ProgramError;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::solana_entrypoint;
use solana_sdk::vote_program::{self, VoteInstruction};

solana_entrypoint!(entrypoint);
fn entrypoint(
    _program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    data: &[u8],
    _tick_height: u64,
) -> Result<(), ProgramError> {
    solana_logger::setup();

    trace!("process_instruction: {:?}", data);
    trace!("keyed_accounts: {:?}", keyed_accounts);

    // all vote instructions require that accounts_keys[0] be a signer
    if keyed_accounts[0].signer_key().is_none() {
        error!("account[0] is unsigned");
        Err(ProgramError::InvalidArgument)?;
    }

    match deserialize(data).map_err(|_| ProgramError::InvalidUserdata)? {
        VoteInstruction::RegisterAccount(node_id) => {
            vote_program::register(keyed_accounts, node_id)
        }
        VoteInstruction::Vote(vote) => {
            debug!("{:?} by {}", vote, keyed_accounts[0].signer_key().unwrap());
            solana_metrics::submit(
                solana_metrics::influxdb::Point::new("vote-native")
                    .add_field("count", solana_metrics::influxdb::Value::Integer(1))
                    .to_owned(),
            );
            vote_program::process_vote(keyed_accounts, vote)
        }
        VoteInstruction::ClearCredits => vote_program::clear_credits(keyed_accounts),
    }
}
