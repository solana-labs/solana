#![cfg(feature = "full")]

use crate::{
    hash::Hash,
    sanitize::Sanitize,
    transaction::{Result, Transaction, TransactionError},
};
use std::{borrow::Cow, convert::TryFrom};

/// Transaction and the hash of its message
#[derive(Debug, Clone)]
pub struct HashedTransaction<'a> {
    transaction: Cow<'a, Transaction>,
    pub message_hash: Hash,
}

impl<'a> HashedTransaction<'a> {
    pub fn try_create(transaction: Cow<'a, Transaction>, message_hash: Hash) -> Result<Self> {
        transaction.sanitize()?;
        if Self::has_duplicates(&transaction.message.account_keys) {
            return Err(TransactionError::AccountLoadedTwice);
        }

        Ok(Self {
            transaction,
            message_hash,
        })
    }

    /// Return true if the slice has any duplicate elements
    pub fn has_duplicates<T: PartialEq>(xs: &[T]) -> bool {
        // Note: This is an O(n^2) algorithm, but requires no heap allocations. The benchmark
        // `bench_has_duplicates` in benches/message_processor.rs shows that this implementation is
        // ~50 times faster than using HashSet for very short slices.
        for i in 1..xs.len() {
            #[allow(clippy::integer_arithmetic)]
            if xs[i..].contains(&xs[i - 1]) {
                return true;
            }
        }
        false
    }

    pub fn transaction(&self) -> &Transaction {
        self.transaction.as_ref()
    }
}

impl<'a> TryFrom<Transaction> for HashedTransaction<'_> {
    type Error = TransactionError;
    fn try_from(transaction: Transaction) -> Result<Self> {
        let message_hash = transaction.message().hash();
        Self::try_create(Cow::Owned(transaction), message_hash)
    }
}

impl<'a> TryFrom<&'a Transaction> for HashedTransaction<'a> {
    type Error = TransactionError;
    fn try_from(transaction: &'a Transaction) -> Result<Self> {
        let message_hash = transaction.message().hash();
        Self::try_create(Cow::Borrowed(transaction), message_hash)
    }
}

pub trait HashedTransactionSlice<'a> {
    fn as_transactions_iter(&'a self) -> Box<dyn Iterator<Item = &'a Transaction> + '_>;
}

impl<'a> HashedTransactionSlice<'a> for [HashedTransaction<'a>] {
    fn as_transactions_iter(&'a self) -> Box<dyn Iterator<Item = &'a Transaction> + '_> {
        Box::new(self.iter().map(|h| h.transaction.as_ref()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_has_duplicates() {
        assert!(!HashedTransaction::has_duplicates(&[1, 2]));
        assert!(HashedTransaction::has_duplicates(&[1, 2, 1]));
    }
}
