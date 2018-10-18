//! This module implements a crdt type designed for asynchronous updates in a distributed network.
//! ContactInfo data type represents the location of distributed nodes in the network which are
//! hosting copies Crdt itself, and ContactInfo is stored in the Crdt itself.  So the Crdt
//! maintains a list of the nodes that are distributing copies of itself.  For all the types
//! replicated in the Crdt, the latest version is picked.  This creates a mutual dependency between
//! Gossip <-> Crdt libraries.
//!
//! Additional types can be added by appending them to the Key, Value enums.
//!
//! Merge strategy `(a: Crdt, b: Crdt) -> Crdt` is implmented in the following steps
//! 1. a has a local Crdt::version A', and it knows of b's remote Crdt::version B
//! 2. b has a local Crdt::version B' and it knows of a's remote Crdt::version A
//! 3. a asynchronosly calls b.get_updates_since(B, max_number_of_updates)
//! 4. b responds with changes to its `table` between B and up to B' and the max version in the
//!    response B''
//! 5. a inserts all the updates that b responded with, records a.version value at which they where
//!    commited into a.table.  It updates a.remote[&b] = B''
//! 6. b does the same
//! 7. eventually the values returned in the updated will be synchronized and no new inserts will
//!    occur in either table
//! 8. get_updates_since will then return 0 updates
//!
//!
//! Each item in the Crdt has its own version.  It's only successfully updated if the update has a
//! newer version then what is currently in the Crdt::table.  So if b had no new updates for a,
//! while it would still transmit data, a's would not update any values in its table and there for
//! wouldn't update its own Crdt::version.
//!
use bincode::serialized_size;
use counter::Counter;
use log::Level;
use solana_program_interface::pubkey::Pubkey;
use std::cmp;
use std::collections::HashMap;
use std::fmt;
use std::net::SocketAddr;
use std::sync::atomic::AtomicUsize;
use transaction::Transaction;

pub struct Crdt {
    /// key value hashmap
    pub table: HashMap<Key, VersionedValue>,

    /// the version of the `table`
    /// every change to `table` should increase this version number
    pub version: u64,
}

/// Structure representing a node on the network
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ContactInfo {
    pub id: Pubkey,
    /// gossip address
    pub ncp: SocketAddr,
    /// address to connect to for replication
    pub tvu: SocketAddr,
    /// address to connect to when this node is leader
    pub rpu: SocketAddr,
    /// transactions address
    pub tpu: SocketAddr,
    /// storage data address
    pub storage_addr: SocketAddr,
    /// latest version picked
    pub version: u64,
}
impl Default for ContactInfo {
    fn default() -> Self {
        let daddr: SocketAddr = "0.0.0.0:0".parse().unwrap();
        ContactInfo {
            id: Pubkey::default(),
            ncp: daddr.clone(),
            tvu: daddr.clone(),
            rpu: daddr.clone(),
            tpu: daddr.clone(),
            storage_addr: daddr.clone(),
            version: 0,
        }
    }
}
/// Key type, after appending to this enum update
/// * Crdt::update_timestamp_for_pubkey
#[derive(PartialEq, Hash, Eq, Clone, Debug)]
pub enum Key {
    ContactInfo(Pubkey),
    Vote(Pubkey),
    LeaderId(Pubkey),
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Key::ContactInfo(_) => write!(f, "ContactInfo({})", self.pubkey()),
            Key::Vote(_) => write!(f, "Vote({})", self.pubkey()),
            Key::LeaderId(_) => write!(f, "LeaderId({})", self.pubkey()),
        }
    }
}

/// Value must correspond to Key
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum Value {
    /// * Merge Strategy - Latest version is picked
    ContactInfo(ContactInfo),
    /// TODO, Transactions need a height!!!
    /// * Merge Strategy - Latest height is picked
    Vote {
        transaction: Transaction,
        height: u64,
    },
    /// * Merge Strategy - Latest version is picked
    LeaderId {
        id: Pubkey,
        leader_id: Pubkey,
        version: u64,
    },
}

#[derive(PartialEq, Debug)]
pub enum CrdtError {
    InsertFailed,
}

pub struct VersionedValue {
    value: Value,
    /// local time when added
    local_timestamp: u64,
    /// local crdt version when added
    local_version: u64,
}

impl Key {
    fn pubkey(&self) -> Pubkey {
        match self {
            Key::ContactInfo(p) => *p,
            Key::Vote(p) => *p,
            Key::LeaderId(p) => *p,
        }
    }
}

impl Value {
    pub fn version(&self) -> u64 {
        match self {
            Value::ContactInfo(n) => n.version,
            Value::Vote {
                transaction: _,
                height,
            } => *height,
            Value::LeaderId {
                id: _,
                leader_id: _,
                version,
            } => *version,
        }
    }
    pub fn id(&self) -> Key {
        match self {
            Value::ContactInfo(n) => Key::ContactInfo(n.id),
            Value::Vote {
                transaction,
                height: _,
            } => Key::Vote(transaction.account_keys[0]),
            Value::LeaderId {
                id,
                leader_id: _,
                version: _,
            } => Key::LeaderId(*id),
        }
    }
}
impl Default for Crdt {
    fn default() -> Self {
        Crdt {
            table: HashMap::new(),
            version: 0,
        }
    }
}

impl Crdt {
    pub fn insert(&mut self, value: Value, local_timestamp: u64) -> Result<(), CrdtError> {
        let key = value.id();
        if self.table.get(&key).is_none() || (value.version() > self.table[&key].value.version()) {
            trace!(
                "insert value.id: {} version: {}",
                value.id(),
                value.version()
            );
            if self.table.get(&value.id()).is_none() {
                inc_new_counter_info!("crdt-insert-new_entry", 1, 1);
            }

            let versioned_value = VersionedValue {
                value,
                local_timestamp,
                local_version: self.version,
            };
            let _ = self.table.insert(key, versioned_value);
            self.version += 1;
            Ok(())
        } else {
            trace!(
                "INSERT FAILED data: {} new.version: {} me.version: {}",
                value.id(),
                value.version(),
                self.table[&value.id()].value.version()
            );
            Err(CrdtError::InsertFailed)
        }
    }

    fn update_timestamp(&mut self, id: Key, now: u64) {
        self.table
            .get_mut(&id)
            .map(|e| e.local_timestamp = cmp::max(e.local_timestamp, now));
    }

    /// Update the timestamp's of all the values that are assosciated with Pubkey
    pub fn update_timestamp_for_pubkey(&mut self, pubkey: Pubkey, now: u64) {
        self.update_timestamp(Key::ContactInfo(pubkey), now);
        self.update_timestamp(Key::Vote(pubkey), now);
        self.update_timestamp(Key::LeaderId(pubkey), now);
    }

    /// find all the keys that are older or equal to min_ts
    pub fn find_old_keys(&self, min_ts: u64) -> Vec<Key> {
        self.table
            .iter()
            .filter_map(|(k, v)| {
                if v.local_timestamp <= min_ts {
                    Some(k)
                } else {
                    None
                }
            }).cloned()
            .collect()
    }

    pub fn remove(&mut self, key: &Key) {
        self.table.remove(key);
    }

    /// Get updated node since min_version up to a maximum of `max_number` of updates
    /// * min_version - return updates greater then min_version
    /// * max_number - max number of update
    /// * remote_versions - The remote `Crdt::version` values for each update.  This is a structure
    /// about the external state of the network that is maintained by the gossip library.
    /// Returns (max version, updates)  
    /// * max version - the maximum version that is in the updates
    /// * updates - a vector of (Values, Value's remote update index) that have been changed.  Values
    /// remote update index is the last update index seen from the ContactInfo that is referenced by
    /// Value::id().pubkey().
    pub fn get_updates_since(
        &self,
        min_version: u64,
        mut max_bytes: usize,
        remote_versions: &HashMap<Pubkey, u64>,
    ) -> (u64, Vec<(Value, u64)>) {
        let mut items: Vec<_> = self
            .table
            .iter()
            .filter_map(|(k, x)| {
                if k.pubkey() != Pubkey::default() && x.local_version >= min_version {
                    let remote = *remote_versions.get(&k.pubkey()).unwrap_or(&0);
                    Some((x, remote))
                } else {
                    None
                }
            }).collect();
        trace!("items length: {}", items.len());
        items.sort_by_key(|k| k.0.local_version);
        let last = {
            let mut last = 0;
            let sz =  serialized_size(&min_version).unwrap() as usize;
            if max_bytes > sz {
                max_bytes -= sz;
            }
            for i in &items {
                let sz = serialized_size(&(&i.0.value, i.1)).unwrap() as usize;
                if max_bytes < sz {
                    break;
                }
                max_bytes -= sz;
                last += 1;
            }
            last
        };
        let last_version = cmp::max(last, 1) - 1;
        let max_update_version = items
            .get(last_version)
            .map(|i| i.0.local_version)
            .unwrap_or(0);
        let updates: Vec<(Value, u64)> = items
            .into_iter()
            .take(last)
            .map(|x| (x.0.value.clone(), x.1))
            .collect();
        (max_update_version, updates)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use signature::{Keypair, KeypairUtil};
    use system_transaction::test_tx;
    #[test]
    fn test_keys_and_values() {
        let v1 = Value::LeaderId {
            id: Pubkey::default(),
            leader_id: Pubkey::default(),
            version: 0,
        };
        let v2 = Value::ContactInfo(ContactInfo::default());
        let v3 = Value::Vote {
            transaction: test_tx(),
            height: 0,
        };
        for v in &[v1, v2, v3] {
            match (v, v.id()) {
                (Value::ContactInfo(_), Key::ContactInfo(_)) => (),
                (Value::Vote { .. }, Key::Vote(_)) => (),
                (Value::LeaderId { .. }, Key::LeaderId(_)) => (),
                _ => assert!(false),
            }
        }
    }
    #[test]
    fn test_insert() {
        let mut crdt = Crdt::default();
        let val = Value::LeaderId {
            id: Pubkey::default(),
            leader_id: Pubkey::default(),
            version: 0,
        };
        assert_eq!(crdt.insert(val.clone(), 0), Ok(()));
        assert_eq!(crdt.version, 1);
        assert_eq!(crdt.table.len(), 1);
        assert!(crdt.table.contains_key(&val.id()));
        assert_eq!(crdt.table[&val.id()].local_timestamp, 0);
        assert_eq!(crdt.table[&val.id()].local_version, crdt.version - 1);
    }
    #[test]
    fn test_update_old() {
        let mut crdt = Crdt::default();
        let val = Value::LeaderId {
            id: Pubkey::default(),
            leader_id: Pubkey::default(),
            version: 0,
        };
        assert_eq!(crdt.insert(val.clone(), 0), Ok(()));
        assert_eq!(crdt.insert(val.clone(), 1), Err(CrdtError::InsertFailed));
        assert_eq!(crdt.table[&val.id()].local_timestamp, 0);
        assert_eq!(crdt.table[&val.id()].local_version, crdt.version - 1);
        assert_eq!(crdt.version, 1);
    }
    #[test]
    fn test_update_new() {
        let mut crdt = Crdt::default();
        let val = Value::LeaderId {
            id: Pubkey::default(),
            leader_id: Pubkey::default(),
            version: 0,
        };
        assert_eq!(crdt.insert(val, 0), Ok(()));
        let val = Value::LeaderId {
            id: Pubkey::default(),
            leader_id: Pubkey::default(),
            version: 1,
        };
        assert_eq!(crdt.insert(val.clone(), 1), Ok(()));
        assert_eq!(crdt.table[&val.id()].local_timestamp, 1);
        assert_eq!(crdt.table[&val.id()].local_version, crdt.version - 1);
        assert_eq!(crdt.version, 2);
    }
    #[test]
    fn test_update_timestsamp() {
        let mut crdt = Crdt::default();
        let val = Value::LeaderId {
            id: Pubkey::default(),
            leader_id: Pubkey::default(),
            version: 0,
        };
        assert_eq!(crdt.insert(val.clone(), 0), Ok(()));

        crdt.update_timestamp(val.id(), 1);
        assert_eq!(crdt.table[&val.id()].local_timestamp, 1);

        crdt.update_timestamp_for_pubkey(val.id().pubkey(), 2);
        assert_eq!(crdt.table[&val.id()].local_timestamp, 2);

        crdt.update_timestamp_for_pubkey(val.id().pubkey(), 1);
        assert_eq!(crdt.table[&val.id()].local_timestamp, 2);
    }
    #[test]
    fn test_find_old_keys() {
        let mut crdt = Crdt::default();
        let val = Value::LeaderId {
            id: Pubkey::default(),
            leader_id: Pubkey::default(),
            version: 0,
        };
        assert_eq!(crdt.insert(val.clone(), 1), Ok(()));

        assert!(crdt.find_old_keys(0).is_empty());
        assert_eq!(crdt.find_old_keys(1), vec![val.id()]);
        assert_eq!(crdt.find_old_keys(2), vec![val.id()]);
    }
    #[test]
    fn test_remove() {
        let mut crdt = Crdt::default();
        let val = Value::LeaderId {
            id: Pubkey::default(),
            leader_id: Pubkey::default(),
            version: 0,
        };
        assert_eq!(crdt.insert(val.clone(), 1), Ok(()));

        assert_eq!(crdt.find_old_keys(1), vec![val.id()]);
        crdt.remove(&val.id());
        assert!(crdt.find_old_keys(1).is_empty());
    }
    #[test]
    fn test_updates_empty() {
        let crdt = Crdt::default();
        let remotes = HashMap::new();
        assert_eq!(crdt.get_updates_since(0, 0, &remotes), (0, vec![]));
        assert_eq!(crdt.get_updates_since(0, 1024, &remotes), (0, vec![]));
    }
    #[test]
    fn test_updates_default_key() {
        let mut crdt = Crdt::default();
        let val = Value::LeaderId {
            id: Pubkey::default(),
            leader_id: Pubkey::default(),
            version: 0,
        };
        assert_eq!(crdt.insert(val.clone(), 1), Ok(()));
        let mut remotes = HashMap::new();
        remotes.insert(val.id().pubkey(), 1);
        assert_eq!(crdt.get_updates_since(0, 1024, &remotes), (0, vec![]));
        assert_eq!(crdt.get_updates_since(0, 0, &remotes), (0, vec![]));
        assert_eq!(crdt.get_updates_since(1, 1024, &remotes), (0, vec![]));
    }
    #[test]
    fn test_updates_real_key() {
        let key = Keypair::new();
        let mut crdt = Crdt::default();
        let val = Value::LeaderId {
            id: key.pubkey(),
            leader_id: key.pubkey(),
            version: 0,
        };
        assert_eq!(crdt.insert(val.clone(), 1), Ok(()));
        let mut remotes = HashMap::new();
        assert_eq!(
            crdt.get_updates_since(0, 1024, &remotes),
            (0, vec![(val.clone(), 0)])
        );
        remotes.insert(val.id().pubkey(), 1);
        let sz = serialized_size(&(0, vec![(val.clone(), 1)])).unwrap() as usize;
        assert_eq!(crdt.get_updates_since(0, sz, &remotes), (0, vec![(val, 1)]));
        assert_eq!(crdt.get_updates_since(0, sz-1, &remotes), (0, vec![]));
        assert_eq!(crdt.get_updates_since(0, 0, &remotes), (0, vec![]));
        assert_eq!(crdt.get_updates_since(1, sz, &remotes), (0, vec![]));
    }
}
