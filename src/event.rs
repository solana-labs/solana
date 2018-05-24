//! The `event` module handles events, which may be a `Transaction`, or a `Witness` used to process a pending
//! Transaction.

use bincode::serialize;
use chrono::prelude::*;
use hash::Hash;
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
    pub fn new_transaction(
        from_keypair: &KeyPair,
        to: PublicKey,
        tokens: i64,
        last_id: Hash,
    ) -> Self {
        let tr = Transaction::new(from_keypair, to, tokens, last_id);
        Event::Transaction(tr)
    }

    /// Create and sign a new Witness Timestamp. Used for unit-testing.
    pub fn new_timestamp(from: &KeyPair, dt: DateTime<Utc>, _last_id: Hash) -> Self {
        let sign_data = serialize(&dt).expect("serialize 'dt' in pub fn new_timestamp");
        let sig = Signature::clone_from_slice(from.sign(&sign_data).as_ref());
        Event::Timestamp {
            from: from.pubkey(),
            dt,
            sig,
        }
    }

    /// Create and sign a new Witness Signature. Used for unit-testing.
    pub fn new_signature(from: &KeyPair, tx_sig: Signature, _last_id: Hash) -> Self {
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
            Event::Timestamp { from, dt, sig } => sig.verify(
                &from,
                &serialize(&dt).expect("serialize 'dt' in pub fn verify"),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use signature::{KeyPair, KeyPairUtil};

    #[test]
    fn test_event_verify() {
        let zero = Hash::default();
        assert!(Event::new_timestamp(&KeyPair::new(), Utc::now(), zero).verify());
        assert!(Event::new_signature(&KeyPair::new(), Signature::default(), zero).verify());
    }
}
