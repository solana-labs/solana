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

    /// Create and sign a new Witness Signature. Used for unit-testing.
    pub fn new_signature(from: &KeyPair, tx_sig: Signature) -> Self {
        let sig = Signature::clone_from_slice(from.sign(&tx_sig).as_ref());
        Event::Signature {
            from: from.pubkey(),
            tx_sig,
            sig,
        }
    }

    /// Verify the Event's signature's are valid and if a transaction, that its
    /// spending plan is valid.
    pub fn verify(&self) -> bool {
        match *self {
            Event::Transaction(ref tr) => tr.verify_sig(),
            Event::Signature { from, tx_sig, sig } => sig.verify(&from, &tx_sig),
            Event::Timestamp { from, dt, sig } => sig.verify(&from, &serialize(&dt).unwrap()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use signature::{KeyPair, KeyPairUtil};

    #[test]
    fn test_event_verify() {
        assert!(Event::new_timestamp(&KeyPair::new(), Utc::now()).verify());
        assert!(Event::new_signature(&KeyPair::new(), Signature::default()).verify());
    }
}
