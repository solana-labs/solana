use serde::de::{Deserialize, Deserializer};
use serde::ser::{Serialize, Serializer};
use std::{
    ops::{Deref, DerefMut, Index, IndexMut},
    slice::SliceIndex,
};

#[derive(Debug, Default)]
pub struct AccountData {
    data: Vec<u8>,
}

impl AccountData {}

impl From<Vec<u8>> for AccountData {
    fn from(data: Vec<u8>) -> Self {
        Self { data }
    }
}

impl From<AccountData> for Vec<u8> {
    fn from(account_data: AccountData) -> Self {
        account_data.data
    }
}

impl Deref for AccountData {
    type Target = Vec<u8>;

    fn deref(&self) -> &Self::Target {
        &self.data
    }
}
impl DerefMut for AccountData {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.data
    }
}

impl AsRef<[u8]> for AccountData {
    fn as_ref(&self) -> &[u8] {
        &self.data
    }
}

impl<I> Index<I> for AccountData
where
    I: SliceIndex<[u8]>,
{
    type Output = I::Output;

    #[inline]
    fn index(&self, index: I) -> &Self::Output {
        self.data.index(index)
    }
}

impl<I> IndexMut<I> for AccountData
where
    I: SliceIndex<[u8]>,
{
    #[inline]
    fn index_mut(&mut self, index: I) -> &mut Self::Output {
        // Invalidate the cache since the data may be mutated.
        self.data.index_mut(index)
    }
}

impl Clone for AccountData {
    fn clone(&self) -> Self {
        Self {
            data: self.data.clone(),
        }
    }
}

impl PartialEq<AccountData> for AccountData {
    fn eq(&self, other: &Self) -> bool {
        self.data == other.data
    }
}

impl Eq for AccountData {}

impl Serialize for AccountData {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serde_bytes::serialize(&self.data, serializer)
    }
}

impl<'de> Deserialize<'de> for AccountData {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let data = serde_bytes::deserialize::<Vec<u8>, D>(deserializer)?;
        Ok(Self::from(data))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        account::{self, accounts_equal, Account},
        account_shared_data::AccountSharedData,
        pubkey::Pubkey,
    };
    use bincode::Options;
    use rand::Rng;
    use solana_program::sysvar::clock::Clock;

    fn new_rand_sysvar_clock<R: Rng>(rng: &mut R) -> Clock {
        Clock {
            slot: rng.gen(),
            epoch_start_timestamp: rng.gen(),
            epoch: rng.gen(),
            leader_schedule_epoch: rng.gen(),
            unix_timestamp: rng.gen(),
        }
    }

    // Asserts that the new Account struct is backward compatible.
    #[test]
    fn test_serialization() {
        let mut rng = rand::thread_rng();
        let clock = new_rand_sysvar_clock(&mut rng);
        let data = bincode::serialize(&clock).unwrap();
        let account = AccountSharedData::from(Account {
            lamports: rng.gen(),
            data: data.clone(),
            owner: Pubkey::new_unique(),
            executable: rng.gen(),
            rent_epoch: rng.gen(),
        });
        let old_account = Account {
            lamports: account.lamports,
            data,
            owner: account.owner,
            executable: account.executable,
            rent_epoch: account.rent_epoch,
        };

        let bytes = bincode::serialize(&old_account).unwrap();
        assert_eq!(bytes, bincode::serialize(&account).unwrap());
        let other_account: Account = bincode::deserialize(&bytes).unwrap();
        assert!(accounts_equal(&account, &other_account));
        assert_eq!(
            account::from_account::<Clock, _>(&other_account).unwrap(),
            clock
        );

        let bytes = bincode::options().serialize(&old_account).unwrap();
        assert_eq!(bytes, bincode::options().serialize(&account).unwrap());
        let other_account: Account = bincode::options().deserialize(&bytes).unwrap();
        assert!(accounts_equal(&account, &other_account));
        assert_eq!(
            account::from_account::<Clock, _>(&other_account).unwrap(),
            clock
        );
    }
}
