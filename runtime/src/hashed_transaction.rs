use solana_sdk::{hash::Hash, transaction::Transaction, vote};
use std::borrow::Cow;

/// Transaction and the hash of its message
#[derive(Debug, Clone)]
pub struct HashedTransaction<'a> {
    transaction: Cow<'a, Transaction>,
    pub message_hash: Hash,
    is_simple_vote_tx: bool,
}

impl<'a> HashedTransaction<'a> {
    pub fn new(
        transaction: Cow<'a, Transaction>,
        message_hash: Hash,
        is_simple_vote_tx: Option<bool>,
    ) -> Self {
        let is_simple_vote_tx = is_simple_vote_tx.unwrap_or_else(|| {
            let message = transaction.message();
            message
                .instructions
                .get(0)
                .and_then(|ix| message.account_keys.get(ix.program_id_index as usize))
                == Some(&vote::program::id())
        });

        Self {
            transaction,
            message_hash,
            is_simple_vote_tx,
        }
    }

    pub fn transaction(&self) -> &Transaction {
        self.transaction.as_ref()
    }

    /// Returns true if this transaction is a simple vote
    pub fn is_simple_vote_transaction(&self) -> bool {
        self.is_simple_vote_tx
    }
}

impl<'a> From<Transaction> for HashedTransaction<'_> {
    fn from(transaction: Transaction) -> Self {
        let message_hash = transaction.message().hash();
        Self::new(Cow::Owned(transaction), message_hash, None)
    }
}

impl<'a> From<&'a Transaction> for HashedTransaction<'a> {
    fn from(transaction: &'a Transaction) -> Self {
        let message_hash = transaction.message().hash();
        Self::new(Cow::Borrowed(transaction), message_hash, None)
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
