//! The `event` crate provides the data structures for log events.

use signature::Signature;
use transaction::Transaction;

/// When 'event' is Tick, the event represents a simple clock tick, and exists for the
/// sole purpose of improving the performance of event log verification. A tick can
/// be generated in 'num_hashes' hashes and verified in 'num_hashes' hashes.  By logging
/// a hash alongside the tick, each tick and be verified in parallel using the 'id'
/// of the preceding tick to seed its hashing.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum Event {
    Tick,
    Transaction(Transaction<i64>),
}

impl Event {
    pub fn get_signature(&self) -> Option<Signature> {
        match *self {
            Event::Tick => None,
            Event::Transaction(ref tr) => Some(tr.sig),
        }
    }

    pub fn verify(&self) -> bool {
        match *self {
            Event::Tick => true,
            Event::Transaction(ref tr) => tr.verify(),
        }
    }
}
