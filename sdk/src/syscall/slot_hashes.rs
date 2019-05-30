//! named accounts for synthesized data accounts for bank state, etc.
//!
//! this account carries the Bank's most recent blockhashes for some N parents
//!
use crate::account::Account;
use crate::hash::Hash;
use crate::pubkey::Pubkey;
use crate::syscall;
use bincode::serialized_size;
use std::ops::Deref;

/// "Sysca11SlotHashes11111111111111111111111111"
///  slot hashes account pubkey
const ID: [u8; 32] = [
    6, 167, 211, 138, 69, 219, 186, 157, 48, 170, 46, 66, 2, 146, 193, 59, 39, 59, 245, 188, 30,
    60, 130, 78, 86, 27, 113, 191, 208, 0, 0, 0,
];

pub fn id() -> Pubkey {
    Pubkey::new(&ID)
}

pub fn check_id(pubkey: &Pubkey) -> bool {
    pubkey.as_ref() == ID
}

pub const MAX_SLOT_HASHES: usize = 512; // 512 slots to get your vote in

#[derive(Serialize, Deserialize, PartialEq, Debug)]
pub struct SlotHashes {
    // non-pub to keep control of size
    inner: Vec<(u64, Hash)>,
}

impl SlotHashes {
    pub fn from(account: &Account) -> Result<Self, bincode::Error> {
        account.deserialize_data()
    }

    pub fn with<T, F>(account: &mut Account, func: F) -> Result<T, Box<std::error::Error>>
    where
        F: FnOnce(&mut Self) -> Result<T, Box<std::error::Error>>,
    {
        account.with_data(func)
    }

    pub fn size_of() -> usize {
        serialized_size(&SlotHashes {
            inner: vec![(0, Hash::default()); MAX_SLOT_HASHES],
        })
        .unwrap() as usize
    }
    pub fn add(&mut self, slot: u64, hash: Hash) {
        self.inner.insert(0, (slot, hash));
        self.inner.truncate(MAX_SLOT_HASHES);
    }
}

impl Deref for SlotHashes {
    type Target = Vec<(u64, Hash)>;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

pub fn create_account(lamports: u64) -> Account {
    Account::new(lamports, SlotHashes::size_of(), &syscall::id())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::hash;

    #[test]
    fn test_id() {
        let ids = [("Sysca11S1otHashes11111111111111111111111111", id())];
        // to get the bytes above:
        //        ids.iter().for_each(|(name, _)| {
        //            dbg!((name, bs58::decode(name).into_vec().unwrap()));
        //        });
        assert!(ids.iter().all(|(name, id)| *name == id.to_string()));
        assert!(check_id(&id()));
    }
    #[derive(Debug, Clone)]
    struct DummyError;
    impl std::error::Error for DummyError {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            None
        }
    }
    impl std::fmt::Display for DummyError {
        fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            write!(f, "dummy")
        }
    }

    #[test]
    fn test_from_with() {
        // not enough space for MAX_SLOT_HASHES
        let mut account = Account::new(0, SlotHashes::size_of() - 1, &syscall::id());

        assert!(SlotHashes::from(&account).is_ok()); // slot_hashes state can vary in size

        let mut bcalled = false;
        assert!(SlotHashes::with(&mut account, |slot_hashes| {
            bcalled = true;
            slot_hashes.add(0, Hash::default());

            Err(Box::new(DummyError))?;

            Ok(())
        })
        .is_err());
        // the closure should have run
        assert!(bcalled);
        // the account data should be untouched
        assert_eq!(
            SlotHashes::from(&account).unwrap(),
            SlotHashes { inner: vec![] }
        );
        // try to push something back that's too large
        let mut bcalled = false;
        assert!(SlotHashes::with(&mut account, |slot_hashes| {
            bcalled = true;
            for i in 0..MAX_SLOT_HASHES {
                slot_hashes.add(
                    i as u64,
                    hash(&[(i >> 24) as u8, (i >> 16) as u8, (i >> 8) as u8, i as u8]),
                );
            }
            Ok(())
        })
        .is_err());
        // the account data should be untouched
        assert_eq!(
            SlotHashes::from(&account).unwrap(),
            SlotHashes { inner: vec![] }
        );

        assert!(bcalled);
    }

    #[test]
    fn test_create_account_add() {
        let lamports = 42;
        let account = create_account(lamports);
        let mut slot_hashes = SlotHashes::from(&account).unwrap();
        assert_eq!(slot_hashes, SlotHashes { inner: vec![] });
        for i in 0..MAX_SLOT_HASHES + 1 {
            slot_hashes.add(
                i as u64,
                hash(&[(i >> 24) as u8, (i >> 16) as u8, (i >> 8) as u8, i as u8]),
            );
        }
        for i in 0..MAX_SLOT_HASHES {
            assert_eq!(slot_hashes[i].0, (MAX_SLOT_HASHES - i) as u64);
        }

        assert_eq!(slot_hashes.len(), MAX_SLOT_HASHES);
    }
}
