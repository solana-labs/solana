use crate::pubkey::Pubkey;
use std::{iter::FromIterator, ops::Deref};

// Vote account address and its active stake for the current epoch
pub type VoteAccountStake = (Pubkey, u64);

#[repr(C)]
#[derive(Serialize, Deserialize, PartialEq, Debug, Default)]
pub struct EpochVoteAccounts(Vec<VoteAccountStake>);

impl EpochVoteAccounts {
    pub fn add(&mut self, vote_account_address: Pubkey, active_stake: u64) {
        match self.binary_search_by(|(probe, _)| vote_account_address.cmp(&probe)) {
            Ok(index) => (self.0)[index] = (vote_account_address, active_stake),
            Err(index) => (self.0).insert(index, (vote_account_address, active_stake)),
        }
    }
    #[allow(clippy::trivially_copy_pass_by_ref)]
    pub fn get(&self, vote_account_address: &Pubkey) -> Option<&u64> {
        self.binary_search_by(|(probe, _)| vote_account_address.cmp(&probe))
            .ok()
            .map(|index| &self[index].1)
    }
    pub fn new(mut vote_account_stakes: Vec<VoteAccountStake>) -> Self {
        vote_account_stakes.sort_by(|a, b| a.0.cmp(&b.0));
        Self(vote_account_stakes)
    }
}

impl FromIterator<(Pubkey, u64)> for EpochVoteAccounts {
    fn from_iter<I: IntoIterator<Item = (Pubkey, u64)>>(iter: I) -> Self {
        let vote_account_stakes: Vec<_> = iter.into_iter().collect();
        Self::new(vote_account_stakes)
    }
}

impl Deref for EpochVoteAccounts {
    type Target = Vec<VoteAccountStake>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test() {
        let p1 = Pubkey::new_unique();
        let p2 = Pubkey::new_unique();
        let p3 = Pubkey::new_unique();
        assert!(p1 < p2);
        assert!(p2 < p3);
        // add accounts in reverse order
        let vote_account_with_stake = vec![(p3, 3), (p2, 2), (p1, 1)];

        let golden_epoch_vote_accounts = EpochVoteAccounts(vec![(p1, 1), (p2, 2), (p3, 3)]);

        let epoch_vote_accounts = EpochVoteAccounts::new(vote_account_with_stake.clone());
        assert_eq!(epoch_vote_accounts, golden_epoch_vote_accounts,);
        assert_eq!(
            vote_account_with_stake
                .into_iter()
                .collect::<EpochVoteAccounts>(),
            golden_epoch_vote_accounts,
        );
    }
}
