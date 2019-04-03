//! Vote program
//! Receive and processes votes from validators

use crate::vote_instruction::VoteInstruction;
use crate::vote_state;
use bincode::deserialize;
use log::*;
use solana_sdk::account::KeyedAccount;
use solana_sdk::instruction::InstructionError;
use solana_sdk::pubkey::Pubkey;

pub fn process_instruction(
    _program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    data: &[u8],
    _tick_height: u64,
) -> Result<(), InstructionError> {
    solana_logger::setup();

    trace!("process_instruction: {:?}", data);
    trace!("keyed_accounts: {:?}", keyed_accounts);

    match deserialize(data).map_err(|_| InstructionError::InvalidInstructionData)? {
        VoteInstruction::InitializeAccount => vote_state::initialize_account(keyed_accounts),
        VoteInstruction::DelegateStake(delegate_id) => {
            vote_state::delegate_stake(keyed_accounts, &delegate_id)
        }
        VoteInstruction::AuthorizeVoter(voter_id) => {
            vote_state::authorize_voter(keyed_accounts, &voter_id)
        }
        VoteInstruction::Vote(vote) => {
            debug!("{:?} by {}", vote, keyed_accounts[0].signer_key().unwrap());
            solana_metrics::submit(
                solana_metrics::influxdb::Point::new("vote-native")
                    .add_field("count", solana_metrics::influxdb::Value::Integer(1))
                    .to_owned(),
            );
            vote_state::process_vote(keyed_accounts, vote)
        }
        VoteInstruction::ClearCredits => vote_state::clear_credits(keyed_accounts),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id;
    use crate::vote_instruction::{self, Vote};
    use crate::vote_state::VoteState;
    use solana_runtime::bank::{Bank, Result};
    use solana_runtime::bank_client::BankClient;
    use solana_sdk::genesis_block::GenesisBlock;
    use solana_sdk::instruction::InstructionError;
    use solana_sdk::message::Message;
    use solana_sdk::pubkey::Pubkey;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::sync_client::SyncClient;
    use solana_sdk::system_instruction;
    use solana_sdk::transaction::TransactionError;

    fn create_bank(lamports: u64) -> (Bank, Keypair) {
        let (genesis_block, mint_keypair) = GenesisBlock::new(lamports);
        let mut bank = Bank::new(&genesis_block);
        bank.add_instruction_processor(id(), process_instruction);
        (bank, mint_keypair)
    }

    fn create_vote_account(
        bank_client: &BankClient,
        from_keypair: &Keypair,
        vote_id: &Pubkey,
        lamports: u64,
    ) -> Result<()> {
        let ixs = vote_instruction::create_account(&from_keypair.pubkey(), vote_id, lamports);
        let message = Message::new(ixs);
        bank_client.send_message(&[from_keypair], message)
    }

    fn create_vote_account_with_delegate(
        bank_client: &BankClient,
        from_keypair: &Keypair,
        vote_keypair: &Keypair,
        delegate_id: &Pubkey,
        lamports: u64,
    ) -> Result<()> {
        let vote_id = vote_keypair.pubkey();
        let mut ixs = vote_instruction::create_account(&from_keypair.pubkey(), &vote_id, lamports);
        let delegate_ix = vote_instruction::delegate_stake(&vote_id, delegate_id);
        ixs.push(delegate_ix);
        let message = Message::new(ixs);
        bank_client.send_message(&[&from_keypair, vote_keypair], message)
    }

    fn submit_vote(
        bank_client: &BankClient,
        vote_keypair: &Keypair,
        tick_height: u64,
    ) -> Result<()> {
        let vote_ix = vote_instruction::vote(&vote_keypair.pubkey(), Vote::new(tick_height));
        bank_client.send_instruction(&vote_keypair, vote_ix)
    }

    #[test]
    fn test_vote_bank_basic() {
        let (bank, from_keypair) = create_bank(10_000);
        let bank_client = BankClient::new(&bank);

        let vote_keypair = Keypair::new();
        let vote_id = vote_keypair.pubkey();

        create_vote_account(&bank_client, &from_keypair, &vote_id, 100).unwrap();
        submit_vote(&bank_client, &vote_keypair, 0).unwrap();

        let vote_account = bank.get_account(&vote_id).unwrap();
        let vote_state = VoteState::deserialize(&vote_account.data).unwrap();
        assert_eq!(vote_state.votes.len(), 1);
    }

    #[test]
    fn test_vote_bank_delegate() {
        let (bank, from_keypair) = create_bank(10_000);
        let vote_keypair = Keypair::new();
        let bank_client = BankClient::new(&bank);
        let delegate_id = Pubkey::new_rand();
        create_vote_account_with_delegate(
            &bank_client,
            &from_keypair,
            &vote_keypair,
            &delegate_id,
            100,
        )
        .unwrap();
    }

    #[test]
    fn test_vote_via_bank_with_no_signature() {
        let (bank, mallory_keypair) = create_bank(10_000);
        let bank_client = BankClient::new(&bank);

        let vote_keypair = Keypair::new();
        let vote_id = vote_keypair.pubkey();

        create_vote_account(&bank_client, &mallory_keypair, &vote_id, 100).unwrap();

        let mallory_id = mallory_keypair.pubkey();
        let mut vote_ix = vote_instruction::vote(&vote_id, Vote::new(0));
        vote_ix.accounts[0].is_signer = false; // <--- attack!! No signer required.

        // Sneak in an instruction so that the transaction is signed but
        // the 0th account in the second instruction is not! The program
        // needs to check that it's signed.
        let move_ix = system_instruction::transfer(&mallory_id, &vote_id, 1);
        let message = Message::new(vec![move_ix, vote_ix]);
        let result = bank_client.send_message(&[&mallory_keypair], message);

        // And ensure there's no vote.
        let vote_account = bank.get_account(&vote_id).unwrap();
        let vote_state = VoteState::deserialize(&vote_account.data).unwrap();
        assert_eq!(vote_state.votes.len(), 0);

        assert_eq!(
            result,
            Err(TransactionError::InstructionError(
                1,
                InstructionError::InvalidArgument
            ))
        );
    }
}
