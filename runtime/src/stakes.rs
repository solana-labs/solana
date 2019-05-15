//! Stakes serve as a cache of stake and vote accounts to derive
//! node stakes
use hashbrown::HashMap;
use solana_sdk::account::Account;
use solana_sdk::pubkey::Pubkey;

#[derive(Default, Clone)]
pub struct Stakes {
    /// vote accounts
    vote_accounts: HashMap<Pubkey, (u64, Account)>,

    /// stake_accounts
    stake_accounts: HashMap<Pubkey, Account>,
}

impl Stakes {
    pub fn is_stake(account: &Account) -> bool {
        solana_vote_api::check_id(&account.owner) || solana_stake_api::check_id(&account.owner)
    }

    pub fn store(&mut self, pubkey: &Pubkey, account: &Account) {
        if solana_vote_api::check_id(&account.owner) {
            if account.lamports != 0 {
                self.vote_accounts
                    .insert(*pubkey, (account.lamports, account.clone()));
            } else {
                self.vote_accounts.remove(pubkey);
            }
        } else if solana_stake_api::check_id(&account.owner) {
            if account.lamports != 0 {
                self.stake_accounts.insert(*pubkey, account.clone());
            } else {
                self.stake_accounts.remove(pubkey);
            }
        }
    }
    pub fn vote_accounts(&self) -> &HashMap<Pubkey, (u64, Account)> {
        &self.vote_accounts
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::pubkey::Pubkey;
    use solana_stake_api::stake_state;
    use solana_vote_api::vote_state::{self, VoteState};

    //  set up some dummies  for a staked node    ( node   (     vote      )  (     stake     ))
    fn create_staked_node_accounts(stake: u64) -> (Pubkey, (Pubkey, Account), (Pubkey, Account)) {
        let vote_id = Pubkey::new_rand();
        let node_id = Pubkey::new_rand();
        let vote_account = vote_state::create_account(&vote_id, &node_id, 0, 1);
        (
            node_id,
            (vote_id, vote_account),
            create_stake_account(stake, &vote_id),
        )
    }

    //   add stake to a vote_id                               (   stake    )
    fn create_stake_account(stake: u64, vote_id: &Pubkey) -> (Pubkey, Account) {
        (
            Pubkey::new_rand(),
            stake_state::create_delegate_stake_account(&vote_id, &VoteState::default(), stake),
        )
    }

    #[test]
    #[ignore]
    fn test_stakes_basic() {
        let mut stakes = Stakes::default();

        let (_node_id, (vote_id, vote_account), (stake_id, stake_account)) =
            create_staked_node_accounts(10);

        stakes.store(&vote_id, &vote_account);
        stakes.store(&stake_id, &stake_account);

        let vote_accounts = stakes.vote_accounts();
        assert!(vote_accounts.get(&vote_id).is_some());
        assert_eq!(vote_accounts.get(&vote_id).unwrap().0, 10);
    }

}
