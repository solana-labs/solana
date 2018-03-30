//! The `event` module handles events, which may be a `Transaction`, or a `Witness` used to process a pending
//! Transaction.

use bincode::serialize;
use chrono::prelude::*;
use signature::{KeyPair, KeyPairUtil, PublicKey, Signature, SignatureUtil};
use transaction::Transaction;

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum Event {
    Transaction(Transaction),
    Signature {
        from: PublicKey,
        tx_sig: Signature,
        sig: Signature,
    },
    Timestamp {
        from: PublicKey,
        dt: DateTime<Utc>,
        sig: Signature,
    },
}

impl Event {
    /// Create and sign a new Witness Timestamp. Used for unit-testing.
    pub fn new_timestamp(from: &KeyPair, dt: DateTime<Utc>) -> Self {
        let sign_data = serialize(&dt).unwrap();
        let sig = Signature::clone_from_slice(from.sign(&sign_data).as_ref());
        Event::Timestamp {
            from: from.pubkey(),
            dt,
            sig,
        }
    }

    // TODO: Rename this to transaction_signature().
    /// If the Event is a Transaction, return its Signature.
    pub fn get_signature(&self) -> Option<Signature> {
        match *self {
            Event::Transaction(ref tr) => Some(tr.sig),
            Event::Signature { .. } | Event::Timestamp { .. } => None,
        }
    }

    /// Verify the Event's signature's are valid and if a transaction, that its
    /// spending plan is valid.
    pub fn verify(&self) -> bool {
        match *self {
            Event::Transaction(ref tr) => tr.verify(),
            Event::Signature { from, tx_sig, sig } => sig.verify(&from, &tx_sig),
            Event::Timestamp { from, dt, sig } => sig.verify(&from, &serialize(&dt).unwrap()),
        }
    }
}
