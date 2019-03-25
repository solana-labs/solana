//! The `vote_transaction` module provides functionality for creating vote transactions.

use crate::vote_instruction::{Vote, VoteInstruction};
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::transaction::Transaction;

pub struct VoteTransaction {}

impl VoteTransaction {
    pub fn new_vote<T: KeypairUtil>(
        staking_account: &Pubkey,
        authorized_voter_keypair: &T,
        slot: u64,
        recent_blockhash: Hash,
        fee: u64,
    ) -> Transaction {
        let ix = VoteInstruction::new_vote(staking_account, Vote { slot });
        Transaction::new_signed_instructions(
            &[authorized_voter_keypair],
            vec![ix],
            recent_blockhash,
            fee,
        )
    }

    /// Fund or create the staking account with lamports
    pub fn new_account(
        from_keypair: &Keypair,
        staker_id: &Pubkey,
        recent_blockhash: Hash,
        lamports: u64,
        fee: u64,
    ) -> Transaction {
        let from_id = from_keypair.pubkey();
        let ixs = VoteInstruction::new_account(&from_id, staker_id, lamports);
        Transaction::new_signed_instructions(&[from_keypair], ixs, recent_blockhash, fee)
    }

    /// Fund or create the staking account with lamports
    pub fn new_account_with_delegate(
        from_keypair: &Keypair,
        voter_keypair: &Keypair,
        delegate_id: &Pubkey,
        recent_blockhash: Hash,
        lamports: u64,
        fee: u64,
    ) -> Transaction {
        let from_id = from_keypair.pubkey();
        let voter_id = voter_keypair.pubkey();

        let mut ixs = VoteInstruction::new_account(&from_id, &voter_id, lamports);
        let delegate_ix = VoteInstruction::new_delegate_stake(&voter_id, &delegate_id);
        ixs.push(delegate_ix);

        Transaction::new_signed_instructions(
            &[from_keypair, voter_keypair],
            ixs,
            recent_blockhash,
            fee,
        )
    }

    /// Choose a voter id to accept signed votes from
    pub fn new_authorize_voter(
        vote_keypair: &Keypair,
        recent_blockhash: Hash,
        authorized_voter_id: &Pubkey,
        fee: u64,
    ) -> Transaction {
        let ix = VoteInstruction::new_authorize_voter(&vote_keypair.pubkey(), authorized_voter_id);
        Transaction::new_signed_instructions(&[vote_keypair], vec![ix], recent_blockhash, fee)
    }

    /// Choose a node id to `delegate` or `assign` this vote account to
    pub fn delegate_vote_account<T: KeypairUtil>(
        vote_keypair: &T,
        recent_blockhash: Hash,
        node_id: &Pubkey,
        fee: u64,
    ) -> Transaction {
        let ix = VoteInstruction::new_delegate_stake(&vote_keypair.pubkey(), node_id);
        Transaction::new_signed_instructions(&[vote_keypair], vec![ix], recent_blockhash, fee)
    }
}
