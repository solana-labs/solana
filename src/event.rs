//! The `event` crate provides the data structures for log events.

use signature::{KeyPair, KeyPairUtil, PublicKey, Signature, SignatureUtil};
use transaction::Transaction;
use chrono::prelude::*;
use bincode::serialize;

/// When 'event' is Tick, the event represents a simple clock tick, and exists for the
/// sole purpose of improving the performance of event log verification. A tick can
/// be generated in 'num_hashes' hashes and verified in 'num_hashes' hashes.  By logging
/// a hash alongside the tick, each tick and be verified in parallel using the 'id'
/// of the preceding tick to seed its hashing.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum Event {
    Tick,
    Transaction(Transaction<i64>),
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
    pub fn get_signature(&self) -> Option<Signature> {
        match *self {
            Event::Tick => None,
            Event::Transaction(ref tr) => Some(tr.sig),
            Event::Signature { .. } => None,
            Event::Timestamp { .. } => None,
        }
    }

    pub fn verify(&self) -> bool {
        match *self {
            Event::Tick => true,
            Event::Transaction(ref tr) => tr.verify(),
            Event::Signature { from, tx_sig, sig } => sig.verify(&from, &tx_sig),
            Event::Timestamp { from, dt, sig } => sig.verify(&from, &serialize(&dt).unwrap()),
        }
    }
}
