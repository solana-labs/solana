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
    use crate::vote_instruction::{Vote, VoteInstruction};
    use crate::vote_script::VoteScript;
    use crate::vote_state::VoteState;
    use solana_runtime::bank::{Bank, Result};
    use solana_runtime::bank_client::BankClient;
    use solana_sdk::genesis_block::GenesisBlock;
    use solana_sdk::instruction::{AccountMeta, Instruction, InstructionError};
    use solana_sdk::pubkey::Pubkey;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::system_instruction::SystemInstruction;
    use solana_sdk::transaction::TransactionError;

    fn create_bank(lamports: u64) -> (Bank, Keypair) {
        let (genesis_block, mint_keypair) = GenesisBlock::new(lamports);
        let mut bank = Bank::new(&genesis_block);
        bank.add_instruction_processor(id(), process_instruction);
        (bank, mint_keypair)
    }

    fn create_vote_account(
        bank_client: &BankClient,
        vote_id: &Pubkey,
        lamports: u64,
    ) -> Result<()> {
        let script = VoteScript::new_account(&bank_client.pubkey(), vote_id, lamports);
        bank_client.process_script(script)
    }

    fn create_vote_account_with_delegate(
        bank_client: &BankClient,
        delegate_id: &Pubkey,
        lamports: u64,
    ) -> Result<()> {
        let vote_id = bank_client.pubkeys()[1];
        let mut script = VoteScript::new_account(&bank_client.pubkey(), &vote_id, lamports);
        let delegate_ix = VoteInstruction::new_delegate_stake(&vote_id, delegate_id);
        script.push(delegate_ix);
        bank_client.process_script(script)
    }

    fn submit_vote(
        bank_client: &BankClient,
        staking_account: &Pubkey,
        tick_height: u64,
    ) -> Result<()> {
        let vote_ix = VoteInstruction::new_vote(staking_account, Vote::new(tick_height));
        bank_client.process_instruction(vote_ix)
    }

    #[test]
    fn test_vote_bank_basic() {
        let (bank, from_keypair) = create_bank(10_000);
        let alice_client = BankClient::new(&bank, from_keypair);

        let vote_keypair = Keypair::new();
        let vote_client = BankClient::new(&bank, vote_keypair);
        let vote_id = vote_client.pubkey();

        create_vote_account(&alice_client, &vote_id, 100).unwrap();
        submit_vote(&vote_client, &vote_id, 0).unwrap();

        let vote_account = bank.get_account(&vote_client.pubkey()).unwrap();
        let vote_state = VoteState::deserialize(&vote_account.data).unwrap();
        assert_eq!(vote_state.votes.len(), 1);
    }

    #[test]
    fn test_vote_bank_delegate() {
        let (bank, from_keypair) = create_bank(10_000);
        let vote_keypair = Keypair::new();
        let alice_and_vote_client =
            BankClient::new_with_keypairs(&bank, vec![from_keypair, vote_keypair]);
        let delegate_id = Keypair::new().pubkey();
        create_vote_account_with_delegate(&alice_and_vote_client, &delegate_id, 100).unwrap();
    }

    #[test]
    fn test_vote_via_bank_with_no_signature() {
        let (bank, from_keypair) = create_bank(10_000);
        let mallory_client = BankClient::new(&bank, from_keypair);

        let vote_keypair = Keypair::new();
        let vote_client = BankClient::new(&bank, vote_keypair);
        let vote_id = vote_client.pubkey();

        create_vote_account(&mallory_client, &vote_id, 100).unwrap();

        let mallory_id = mallory_client.pubkey();
        let vote_ix = Instruction::new(
            id(),
            &VoteInstruction::Vote(Vote::new(0)),
            vec![AccountMeta::new(vote_id, false)], // <--- attack!! No signer required.
        );

        // Sneak in an instruction so that the transaction is signed but
        // the 0th account in the second instruction is not! The program
        // needs to check that it's signed.
        let move_ix = SystemInstruction::new_move(&mallory_id, &vote_id, 1);
        let result = mallory_client.process_instructions(vec![move_ix, vote_ix]);

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
