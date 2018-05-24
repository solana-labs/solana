//! The `event` module handles events, which may be a `Transaction`, or a `Witness` used to process a pending
//! Transaction.

use hash::Hash;
use signature::{KeyPair, PublicKey};
use transaction::Transaction;

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum Event {
    Transaction(Transaction),
}

impl Event {
    pub fn new_transaction(
        from_keypair: &KeyPair,
        to: PublicKey,
        tokens: i64,
        last_id: Hash,
    ) -> Self {
        let tr = Transaction::new(from_keypair, to, tokens, last_id);
        Event::Transaction(tr)
    }

    /// Verify the Event's signature's are valid and if a transaction, that its
    /// spending plan is valid.
    pub fn verify(&self) -> bool {
        match *self {
            Event::Transaction(ref tr) => tr.verify_sig(),
        }
    }
}
