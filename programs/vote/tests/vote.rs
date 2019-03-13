use solana_runtime::bank::{Bank, Result};
use solana_sdk::genesis_block::GenesisBlock;
use solana_sdk::hash::hash;
use solana_sdk::native_program::ProgramError;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::system_instruction::SystemInstruction;
use solana_sdk::transaction::{InstructionError, TransactionError};
use solana_sdk::transaction_builder::{BuilderInstruction, TransactionBuilder};
use solana_vote_api::vote_instruction::{Vote, VoteInstruction};
use solana_vote_api::vote_state::VoteState;
use solana_vote_api::vote_transaction::VoteTransaction;

struct VoteBank<'a> {
    bank: &'a Bank,
}

impl<'a> VoteBank<'a> {
    fn new(bank: &'a Bank) -> Self {
        bank.add_native_program("solana_vote_program", &solana_vote_api::id());
        Self { bank }
    }

    fn create_vote_account(
        &self,
        from_keypair: &Keypair,
        vote_id: &Pubkey,
        lamports: u64,
    ) -> Result<()> {
        let blockhash = self.bank.last_blockhash();
        let tx = VoteTransaction::new_account(from_keypair, vote_id, blockhash, lamports, 0);
        self.bank.process_transaction(&tx)
    }

    fn create_vote_account_with_delegate(
        &self,
        from_keypair: &Keypair,
        vote_keypair: &Keypair,
        delegate_id: &Pubkey,
        lamports: u64,
    ) -> Result<()> {
        let blockhash = self.bank.last_blockhash();
        let tx = VoteTransaction::new_account_with_delegate(
            from_keypair,
            vote_keypair,
            delegate_id,
            blockhash,
            lamports,
            0,
        );
        self.bank.process_transaction(&tx)
    }

    fn submit_vote(
        &self,
        staking_account: &Pubkey,
        vote_keypair: &Keypair,
        tick_height: u64,
    ) -> Result<VoteState> {
        let blockhash = self.bank.last_blockhash();
        let tx =
            VoteTransaction::new_vote(staking_account, vote_keypair, tick_height, blockhash, 0);
        self.bank.process_transaction(&tx)?;
        self.bank.register_tick(&hash(blockhash.as_ref()));

        let vote_account = self.bank.get_account(&vote_keypair.pubkey()).unwrap();
        Ok(VoteState::deserialize(&vote_account.userdata).unwrap())
    }
}

#[test]
fn test_vote_bank_basic() {
    let (genesis_block, from_keypair) = GenesisBlock::new(10_000);
    let bank = Bank::new(&genesis_block);
    let vote_bank = VoteBank::new(&bank);

    let vote_keypair = Keypair::new();
    let vote_id = vote_keypair.pubkey();
    vote_bank
        .create_vote_account(&from_keypair, &vote_id, 100)
        .unwrap();

    let vote_state = vote_bank.submit_vote(&vote_id, &vote_keypair, 0).unwrap();
    assert_eq!(vote_state.votes.len(), 1);
}

#[test]
fn test_vote_bank_delegate() {
    let (genesis_block, from_keypair) = GenesisBlock::new(10_000);
    let bank = Bank::new(&genesis_block);
    let vote_bank = VoteBank::new(&bank);
    let vote_keypair = Keypair::new();
    let delegate_keypair = Keypair::new();
    let delegate_id = delegate_keypair.pubkey();
    vote_bank
        .create_vote_account_with_delegate(&from_keypair, &vote_keypair, &delegate_id, 100)
        .unwrap();
}

#[test]
fn test_vote_via_bank_with_no_signature() {
    let (genesis_block, mallory_keypair) = GenesisBlock::new(10_000);
    let bank = Bank::new(&genesis_block);
    let vote_bank = VoteBank::new(&bank);

    let vote_keypair = Keypair::new();
    let vote_id = vote_keypair.pubkey();
    vote_bank
        .create_vote_account(&mallory_keypair, &vote_id, 100)
        .unwrap();

    let mallory_id = mallory_keypair.pubkey();
    let blockhash = bank.last_blockhash();
    let vote_ix = BuilderInstruction::new(
        solana_vote_api::id(),
        &VoteInstruction::Vote(Vote::new(0)),
        vec![(vote_id, false)], // <--- attack!! No signature.
    );

    // Sneak in an instruction so that the transaction is signed but
    // the 0th account in the second instruction is not! The program
    // needs to check that it's signed.
    let tx = TransactionBuilder::default()
        .push(SystemInstruction::new_move(&mallory_id, &vote_id, 1))
        .push(vote_ix)
        .sign(&[&mallory_keypair], blockhash);

    let result = bank.process_transaction(&tx);

    // And ensure there's no vote.
    let vote_account = bank.get_account(&vote_id).unwrap();
    let vote_state = VoteState::deserialize(&vote_account.userdata).unwrap();
    assert_eq!(vote_state.votes.len(), 0);

    assert_eq!(
        result,
        Err(TransactionError::InstructionError(
            1,
            InstructionError::ProgramError(ProgramError::InvalidArgument)
        ))
    );
}
