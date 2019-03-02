use solana_rewards_api;
use solana_rewards_api::rewards_transaction::RewardsTransaction;
use solana_runtime::bank::{Bank, Result};
use solana_sdk::genesis_block::GenesisBlock;
use solana_sdk::hash::hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::vote_program::{self, VoteState};
use solana_sdk::vote_transaction::VoteTransaction;

struct RewardsBank<'a> {
    bank: &'a Bank,
}

impl<'a> RewardsBank<'a> {
    fn new(bank: &'a Bank) -> Self {
        bank.add_native_program("solana_rewards_program", &solana_rewards_api::id());
        Self { bank }
    }

    fn create_rewards_account(
        &self,
        from_keypair: &Keypair,
        rewards_id: Pubkey,
        lamports: u64,
    ) -> Result<()> {
        let block_hash = self.bank.last_block_hash();
        let tx = RewardsTransaction::new_account(from_keypair, rewards_id, block_hash, lamports, 0);
        self.bank.process_transaction(&tx)
    }

    fn create_vote_account(
        &self,
        from_keypair: &Keypair,
        vote_id: Pubkey,
        lamports: u64,
    ) -> Result<()> {
        let block_hash = self.bank.last_block_hash();
        let tx =
            VoteTransaction::fund_staking_account(from_keypair, vote_id, block_hash, lamports, 0);
        self.bank.process_transaction(&tx)
    }

    fn submit_vote(&self, vote_keypair: &Keypair, tick_height: u64) -> Result<VoteState> {
        let block_hash = self.bank.last_block_hash();
        let tx = VoteTransaction::new_vote(vote_keypair, tick_height, block_hash, 0);
        self.bank.process_transaction(&tx)?;
        self.bank.register_tick(&hash(block_hash.as_ref()));

        let vote_account = self.bank.get_account(&vote_keypair.pubkey()).unwrap();
        Ok(VoteState::deserialize(&vote_account.userdata).unwrap())
    }

    fn redeem_credits(&self, rewards_id: Pubkey, vote_keypair: &Keypair) -> Result<VoteState> {
        let block_hash = self.bank.last_block_hash();
        let tx = RewardsTransaction::new_redeem_credits(&vote_keypair, rewards_id, block_hash, 0);
        self.bank.process_transaction(&tx)?;
        let vote_account = self.bank.get_account(&vote_keypair.pubkey()).unwrap();
        Ok(VoteState::deserialize(&vote_account.userdata).unwrap())
    }
}

#[test]
fn test_redeem_vote_credits_via_bank() {
    let (genesis_block, from_keypair) = GenesisBlock::new(10_000);
    let bank = Bank::new(&genesis_block);
    let rewards_bank = RewardsBank::new(&bank);

    // Create a rewards account to hold all rewards pool tokens.
    let rewards_keypair = Keypair::new();
    let rewards_id = rewards_keypair.pubkey();
    rewards_bank
        .create_rewards_account(&from_keypair, rewards_id, 100)
        .unwrap();

    // A staker create a vote account account and delegates a validator to vote on its behalf.
    let vote_keypair = Keypair::new();
    let vote_id = vote_keypair.pubkey();
    rewards_bank
        .create_vote_account(&from_keypair, vote_id, 100)
        .unwrap();

    // The validator submits votes to accumulate credits.
    for i in 0..vote_program::MAX_LOCKOUT_HISTORY {
        let vote_state = rewards_bank.submit_vote(&vote_keypair, i as u64).unwrap();
        assert_eq!(vote_state.credits(), 0);
    }
    let vote_state = rewards_bank
        .submit_vote(&vote_keypair, vote_program::MAX_LOCKOUT_HISTORY as u64 + 1)
        .unwrap();
    assert_eq!(vote_state.credits(), 1);

    // TODO: Add VoteInstruction::RegisterStakerId so that we don't need to point the "to"
    // account to the "from" account.
    let to_id = vote_id;
    let to_tokens = bank.get_balance(&vote_id);

    // Periodically, the staker sumbits its vote account to the rewards pool
    // to exchange its credits for lamports.
    let vote_state = rewards_bank
        .redeem_credits(rewards_id, &vote_keypair)
        .unwrap();
    assert!(bank.get_balance(&to_id) > to_tokens);
    assert_eq!(vote_state.credits(), 0);
}
