//! The `rewards_transaction` module provides functionality for creating a global
//! rewards account and enabling stakers to redeem credits from their vote accounts.

use crate::rewards_instruction::RewardsInstruction;
use crate::rewards_program;
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Keypair;
use solana_sdk::system_transaction::SystemTransaction;
use solana_sdk::transaction::Transaction;

pub struct RewardsTransaction {}

impl RewardsTransaction {
    pub fn new_account(
        from_keypair: &Keypair,
        rewards_id: Pubkey,
        last_id: Hash,
        num_tokens: u64,
        fee: u64,
    ) -> Transaction {
        SystemTransaction::new_program_account(
            from_keypair,
            rewards_id,
            last_id,
            num_tokens,
            rewards_program::get_max_size() as u64,
            rewards_program::id(),
            fee,
        )
    }

    pub fn new_redeem_credits(
        keypair: &Keypair,
        vote_id: Pubkey,
        to_id: Pubkey,
        last_id: Hash,
        fee: u64,
    ) -> Transaction {
        let instruction = RewardsInstruction::RedeemVoteCredits;
        Transaction::new(
            keypair,
            &[vote_id, to_id],
            rewards_program::id(),
            &instruction,
            last_id,
            fee,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rewards_program;
    //use solana_runtime::execute_transaction;
    use solana_sdk::hash::Hash;
    use solana_sdk::native_loader::create_program_account;
    use solana_sdk::signature::KeypairUtil;
    use solana_sdk::system_program;
    use solana_sdk::vote_program;

    #[test]
    fn test_execute_rewards_transaction() {
        let system_program_account = create_program_account("solana_system_program");
        let mut _new_account_loaders = vec![vec![(system_program::id(), system_program_account)]];
        let from_keypair = Keypair::new();
        let rewards_id = Keypair::new().pubkey();
        let last_id = Hash::default();
        let _tx = RewardsTransaction::new_account(&from_keypair, rewards_id, last_id, 10_000, 0);
        //execute_transaction(&tx, &mut new_account_loaders[..], accounts, 0).unwrap();

        let vote_program_account = create_program_account("solana_vote_program");
        let rewards_program_account = create_program_account("solana_rewards_program");
        let mut _new_redeem_credits_loaders = vec![
            vec![(rewards_program::id(), rewards_program_account)],
            vec![(vote_program::id(), vote_program_account)],
        ];

        let vote_id = Keypair::new().pubkey();
        let to_id = from_keypair.pubkey();
        //let to_tokens = to_account.tokens;
        let _tx = RewardsTransaction::new_redeem_credits(&from_keypair, vote_id, to_id, last_id, 0);
        //execute_transaction(&tx, &mut new_redeem_credits_loaders[..], accounts, 0).unwrap();
        //assert!(to_account.tokens > to_tokens);
    }
}
