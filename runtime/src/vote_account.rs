use serde::de::{Deserialize, Deserializer};
use serde::ser::{Serialize, Serializer};
use solana_sdk::{account::Account, instruction::InstructionError};
use solana_vote_program::vote_state::VoteState;
use std::ops::Deref;
use std::sync::{Arc, Once, RwLock, RwLockReadGuard};

// The value here does not matter. It will be overwritten
// at the first call to VoteAccount::vote_state().
const INVALID_VOTE_STATE: Result<VoteState, InstructionError> =
    Err(InstructionError::InvalidAccountData);

#[derive(Clone, Debug, Default, PartialEq, AbiExample)]
pub struct ArcVoteAccount(Arc<VoteAccount>);

#[derive(Debug, AbiExample)]
pub struct VoteAccount {
    account: Account,
    vote_state: RwLock<Result<VoteState, InstructionError>>,
    vote_state_once: Once,
}

impl VoteAccount {
    pub fn lamports(&self) -> u64 {
        self.account.lamports
    }

    pub fn vote_state(&self) -> RwLockReadGuard<Result<VoteState, InstructionError>> {
        self.vote_state_once.call_once(|| {
            *self.vote_state.write().unwrap() = VoteState::deserialize(&self.account.data);
        });
        self.vote_state.read().unwrap()
    }
}

impl Deref for ArcVoteAccount {
    type Target = VoteAccount;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl Serialize for ArcVoteAccount {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.account.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ArcVoteAccount {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let account = Account::deserialize(deserializer)?;
        Ok(Self::from(account))
    }
}

impl From<Account> for ArcVoteAccount {
    fn from(account: Account) -> Self {
        Self(Arc::new(VoteAccount::from(account)))
    }
}

impl From<Account> for VoteAccount {
    fn from(account: Account) -> Self {
        Self {
            account,
            vote_state: RwLock::new(INVALID_VOTE_STATE),
            vote_state_once: Once::new(),
        }
    }
}

impl Default for VoteAccount {
    fn default() -> Self {
        Self {
            account: Account::default(),
            vote_state: RwLock::new(INVALID_VOTE_STATE),
            vote_state_once: Once::new(),
        }
    }
}

impl PartialEq<VoteAccount> for VoteAccount {
    fn eq(&self, other: &Self) -> bool {
        self.account == other.account
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::Rng;
    use solana_sdk::{pubkey::Pubkey, sysvar::clock::Clock};
    use solana_vote_program::vote_state::{VoteInit, VoteStateVersions};

    fn new_rand_vote_account<R: Rng>(rng: &mut R) -> (Account, VoteState) {
        let vote_init = VoteInit {
            node_pubkey: Pubkey::new_unique(),
            authorized_voter: Pubkey::new_unique(),
            authorized_withdrawer: Pubkey::new_unique(),
            commission: rng.gen(),
        };
        let clock = Clock {
            slot: rng.gen(),
            epoch_start_timestamp: rng.gen(),
            epoch: rng.gen(),
            leader_schedule_epoch: rng.gen(),
            unix_timestamp: rng.gen(),
        };
        let vote_state = VoteState::new(&vote_init, &clock);
        let account = Account::new_data(
            rng.gen(), // lamports
            &VoteStateVersions::Current(Box::new(vote_state.clone())),
            &Pubkey::new_unique(), // owner
        )
        .unwrap();
        (account, vote_state)
    }

    #[test]
    fn test_vote_account() {
        let mut rng = rand::thread_rng();
        let (account, vote_state) = new_rand_vote_account(&mut rng);
        let lamports = account.lamports;
        let vote_account = ArcVoteAccount::from(account);
        assert_eq!(lamports, vote_account.lamports());
        assert_eq!(vote_state, *vote_account.vote_state().as_ref().unwrap());
        // 2nd call to .vote_state() should return the cached value.
        assert_eq!(vote_state, *vote_account.vote_state().as_ref().unwrap());
    }

    #[test]
    fn test_vote_account_serialize() {
        let mut rng = rand::thread_rng();
        let (account, vote_state) = new_rand_vote_account(&mut rng);
        let vote_account = ArcVoteAccount::from(account.clone());
        assert_eq!(vote_state, *vote_account.vote_state().as_ref().unwrap());
        // Assert than ArcVoteAccount has the same wire format as Account.
        assert_eq!(
            bincode::serialize(&account).unwrap(),
            bincode::serialize(&vote_account).unwrap()
        );
    }

    #[test]
    fn test_vote_account_deserialize() {
        let mut rng = rand::thread_rng();
        let (account, vote_state) = new_rand_vote_account(&mut rng);
        let data = bincode::serialize(&account).unwrap();
        let vote_account = ArcVoteAccount::from(account);
        assert_eq!(vote_state, *vote_account.vote_state().as_ref().unwrap());
        let other_vote_account: ArcVoteAccount = bincode::deserialize(&data).unwrap();
        assert_eq!(vote_account, other_vote_account);
        assert_eq!(
            vote_state,
            *other_vote_account.vote_state().as_ref().unwrap()
        );
    }

    #[test]
    fn test_vote_account_round_trip() {
        let mut rng = rand::thread_rng();
        let (account, vote_state) = new_rand_vote_account(&mut rng);
        let vote_account = ArcVoteAccount::from(account);
        assert_eq!(vote_state, *vote_account.vote_state().as_ref().unwrap());
        let data = bincode::serialize(&vote_account).unwrap();
        let other_vote_account: ArcVoteAccount = bincode::deserialize(&data).unwrap();
        // Assert that serialize->deserialized returns the same ArcVoteAccount.
        assert_eq!(vote_account, other_vote_account);
        assert_eq!(
            vote_state,
            *other_vote_account.vote_state().as_ref().unwrap()
        );
    }
}
