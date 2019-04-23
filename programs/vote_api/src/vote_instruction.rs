//! Vote program
//! Receive and processes votes from validators

use crate::id;
use crate::vote_state::{self, Vote, VoteState};
use bincode::deserialize;
use log::*;
use serde_derive::{Deserialize, Serialize};
use solana_sdk::account::KeyedAccount;
use solana_sdk::instruction::{AccountMeta, Instruction, InstructionError};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::system_instruction;

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum VoteInstruction {
    /// Initialize the VoteState for this `vote account`
    /// takes a node_id and commission
    InitializeAccount(Pubkey, u32),

    /// Authorize a voter to send signed votes.
    AuthorizeVoter(Pubkey),

    /// A Vote instruction with recent votes
    Vote(Vec<Vote>),
}

fn initialize_account(vote_id: &Pubkey, node_id: &Pubkey, commission: u32) -> Instruction {
    let account_metas = vec![AccountMeta::new(*vote_id, false)];
    Instruction::new(
        id(),
        &VoteInstruction::InitializeAccount(*node_id, commission),
        account_metas,
    )
}

pub fn create_account(
    from_id: &Pubkey,
    vote_id: &Pubkey,
    node_id: &Pubkey,
    commission: u32,
    lamports: u64,
) -> Vec<Instruction> {
    let space = VoteState::size_of() as u64;
    let create_ix = system_instruction::create_account(&from_id, vote_id, lamports, space, &id());
    let init_ix = initialize_account(vote_id, node_id, commission);
    vec![create_ix, init_ix]
}

pub fn authorize_voter(vote_id: &Pubkey, authorized_voter_id: &Pubkey) -> Instruction {
    let account_metas = vec![AccountMeta::new(*vote_id, true)];
    Instruction::new(
        id(),
        &VoteInstruction::AuthorizeVoter(*authorized_voter_id),
        account_metas,
    )
}

pub fn vote(vote_id: &Pubkey, recent_votes: Vec<Vote>) -> Instruction {
    let account_metas = vec![AccountMeta::new(*vote_id, true)];
    Instruction::new(id(), &VoteInstruction::Vote(recent_votes), account_metas)
}

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
        VoteInstruction::InitializeAccount(node_id, commission) => {
            let mut vote_account = &mut keyed_accounts[0];
            vote_state::initialize_account(&mut vote_account, &node_id, commission)
        }
        VoteInstruction::AuthorizeVoter(voter_id) => {
            let (vote_account, other_signers) = keyed_accounts.split_at_mut(1);
            let vote_account = &mut vote_account[0];

            vote_state::authorize_voter(vote_account, other_signers, &voter_id)
        }
        VoteInstruction::Vote(vote) => {
            solana_metrics::submit(
                solana_metrics::influxdb::Point::new("vote-native")
                    .add_field("count", solana_metrics::influxdb::Value::Integer(1))
                    .to_owned(),
            );
            let (vote_account, other_signers) = keyed_accounts.split_at_mut(1);
            let vote_account = &mut vote_account[0];

            vote_state::process_vote(vote_account, other_signers, &vote)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id;
    use crate::vote_instruction;
    use crate::vote_state::{Vote, VoteState};
    use solana_runtime::bank::Bank;
    use solana_runtime::bank_client::BankClient;
    use solana_sdk::client::SyncClient;
    use solana_sdk::genesis_block::GenesisBlock;
    use solana_sdk::instruction::InstructionError;
    use solana_sdk::message::Message;
    use solana_sdk::pubkey::Pubkey;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::system_instruction;
    use solana_sdk::transaction::{Result, TransactionError};

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
        let ixs = vote_instruction::create_account(
            &from_keypair.pubkey(),
            vote_id,
            &Pubkey::new_rand(),
            0,
            lamports,
        );
        let message = Message::new(ixs);
        bank_client
            .send_message(&[from_keypair], message)
            .map_err(|err| err.unwrap())?;
        Ok(())
    }

    fn submit_vote(
        bank_client: &BankClient,
        vote_keypair: &Keypair,
        tick_height: u64,
    ) -> Result<()> {
        let vote_ix = vote_instruction::vote(&vote_keypair.pubkey(), vec![Vote::new(tick_height)]);
        bank_client
            .send_instruction(vote_keypair, vote_ix)
            .map_err(|err| err.unwrap())?;
        Ok(())
    }

    #[test]
    fn test_vote_bank_basic() {
        let (bank, from_keypair) = create_bank(10_000);
        let bank_client = BankClient::new(bank);

        let vote_keypair = Keypair::new();
        let vote_id = vote_keypair.pubkey();

        create_vote_account(&bank_client, &from_keypair, &vote_id, 100).unwrap();
        submit_vote(&bank_client, &vote_keypair, 0).unwrap();

        let vote_account_data = bank_client.get_account_data(&vote_id).unwrap().unwrap();
        let vote_state = VoteState::deserialize(&vote_account_data).unwrap();
        assert_eq!(vote_state.votes.len(), 1);
    }

    #[test]
    fn test_vote_via_bank_authorize_voter() {
        let (bank, mallory_keypair) = create_bank(10_000);
        let bank_client = BankClient::new(bank);

        let vote_keypair = Keypair::new();
        let vote_id = vote_keypair.pubkey();

        create_vote_account(&bank_client, &mallory_keypair, &vote_id, 100).unwrap();

        let mallory_id = mallory_keypair.pubkey();
        let vote_ix = vote_instruction::authorize_voter(&vote_id, &mallory_id);

        let message = Message::new(vec![vote_ix]);
        assert!(bank_client.send_message(&[&vote_keypair], message).is_ok());
    }

    #[test]
    fn test_vote_via_bank_with_no_signature() {
        let (bank, mallory_keypair) = create_bank(10_000);
        let bank_client = BankClient::new(bank);

        let vote_keypair = Keypair::new();
        let vote_id = vote_keypair.pubkey();

        create_vote_account(&bank_client, &mallory_keypair, &vote_id, 100).unwrap();

        let mallory_id = mallory_keypair.pubkey();
        let mut vote_ix = vote_instruction::vote(&vote_id, vec![Vote::new(0)]);
        vote_ix.accounts[0].is_signer = false; // <--- attack!! No signer required.

        // Sneak in an instruction so that the transaction is signed but
        // the 0th account in the second instruction is not! The program
        // needs to check that it's signed.
        let transfer_ix = system_instruction::transfer(&mallory_id, &vote_id, 1);
        let message = Message::new(vec![transfer_ix, vote_ix]);
        let result = bank_client.send_message(&[&mallory_keypair], message);

        // And ensure there's no vote.
        let vote_account_data = bank_client.get_account_data(&vote_id).unwrap().unwrap();
        let vote_state = VoteState::deserialize(&vote_account_data).unwrap();
        assert_eq!(vote_state.votes.len(), 0);

        assert_eq!(
            result.unwrap_err().unwrap(),
            TransactionError::InstructionError(1, InstructionError::MissingRequiredSignature)
        );
    }
}
