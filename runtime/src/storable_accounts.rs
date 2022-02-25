//! trait for abstracting underlying storage of pubkey and account pairs to be written
use solana_sdk::{account::ReadableAccount, clock::Slot, pubkey::Pubkey};

/// abstract access to pubkey, account, slot, target_slot of either:
/// a. (slot, &[&Pubkey, &ReadableAccount])
/// b. (slot, &[&Pubkey, &ReadableAccount, Slot]) (we will use this later)
/// This trait avoids having to allocate redundant data when there is a duplicated slot parameter.
/// All legacy callers do not have a unique slot per account to store.
pub trait StorableAccounts<'a, T: ReadableAccount + Sync>: Sync {
    /// pubkey at 'index'
    fn pubkey(&self, index: usize) -> &Pubkey;
    /// account at 'index'
    fn account(&self, index: usize) -> &T;
    // current slot for account at 'index'
    fn slot(&self, index: usize) -> Slot;
    /// slot that all accounts are to be written to
    fn target_slot(&self) -> Slot;
    /// true if no accounts to write
    fn is_empty(&self) -> bool;
    /// # accounts to write
    fn len(&self) -> usize;
}

impl<'a, T: ReadableAccount + Sync> StorableAccounts<'a, T> for (Slot, &'a [(&'a Pubkey, &'a T)]) {
    fn pubkey(&self, index: usize) -> &Pubkey {
        self.1[index].0
    }
    fn account(&self, index: usize) -> &T {
        self.1[index].1
    }
    fn slot(&self, _index: usize) -> Slot {
        // per-index slot is not unique per slot when per-account slot is not included in the source data
        self.target_slot()
    }
    fn target_slot(&self) -> Slot {
        self.0
    }
    fn is_empty(&self) -> bool {
        self.1.is_empty()
    }
    fn len(&self) -> usize {
        self.1.len()
    }
}

/// this tuple contains slot info PER account
impl<'a, T: ReadableAccount + Sync> StorableAccounts<'a, T>
    for (Slot, &'a [(&'a Pubkey, &'a T, Slot)])
{
    fn pubkey(&self, index: usize) -> &Pubkey {
        self.1[index].0
    }
    fn account(&self, index: usize) -> &T {
        self.1[index].1
    }
    fn slot(&self, index: usize) -> Slot {
        // note that this could be different than 'target_slot()' PER account
        self.1[index].2
    }
    fn target_slot(&self) -> Slot {
        self.0
    }
    fn is_empty(&self) -> bool {
        self.1.is_empty()
    }
    fn len(&self) -> usize {
        self.1.len()
    }
}

#[cfg(test)]
pub mod tests {
    use {
        super::*,
        solana_sdk::account::{AccountSharedData, WritableAccount},
    };

    fn compare<'a, T: ReadableAccount + Sync + PartialEq + std::fmt::Debug>(
        a: &impl StorableAccounts<'a, T>,
        b: &impl StorableAccounts<'a, T>,
    ) {
        assert_eq!(a.target_slot(), b.target_slot());
        assert_eq!(a.len(), b.len());
        assert_eq!(a.is_empty(), b.is_empty());
        (0..a.len()).into_iter().for_each(|i| {
            assert_eq!(a.pubkey(i), b.pubkey(i));
            assert_eq!(a.account(i), b.account(i));
        })
    }

    #[test]
    fn test_storable_accounts() {
        let max_slots = 3_u64;
        for target_slot in 0..max_slots {
            for entries in 0..2 {
                for starting_slot in 0..max_slots {
                    let mut raw = Vec::new();
                    for entry in 0..entries {
                        let pk = Pubkey::new(&[entry; 32]);
                        raw.push((
                            pk,
                            AccountSharedData::create(
                                ((entry as u64) * starting_slot) as u64,
                                Vec::default(),
                                Pubkey::default(),
                                false,
                                0,
                            ),
                            starting_slot % max_slots,
                        ));
                    }
                    let mut two = Vec::new();
                    let mut three = Vec::new();
                    raw.iter().for_each(|raw| {
                        two.push((&raw.0, &raw.1)); // 2 item tuple
                        three.push((&raw.0, &raw.1, raw.2)); // 3 item tuple, including slot
                    });
                    let test2 = (target_slot, &two[..]);
                    let test3 = (target_slot, &three[..]);
                    compare(&test2, &test3);
                    for (i, raw) in raw.iter().enumerate() {
                        assert_eq!(raw.0, *test3.pubkey(i));
                        assert_eq!(raw.1, *test3.account(i));
                        assert_eq!(raw.2, test3.slot(i));
                        assert_eq!(target_slot, test3.target_slot());
                        assert_eq!(target_slot, test2.slot(i));
                    }
                }
            }
        }
    }
}
