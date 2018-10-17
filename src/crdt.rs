//! This module implements a crdt type designed for asynchronous updates in a distributed network.
//! ContactInfo data type represents the location of distributed nodes in the network which are
//! hosting copies Crdt itself, and ContactInfo is stored in the Crdt itself.  So the Crdt
//! maintains a list of the nodes that are distributing copies of itself.
//!
//! Additional types can be added by appending them to the Key, Value enums.
//! 
//! merge(a: Crdt, b: Crdt) -> Crdt is implmented in the following steps
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
use timing::{duration_as_ms, timestamp};
use cluster_info::{ClusterInfo};
use transaction::{Transaction};

/// Key type, after appending to this enum update
/// * Crdt::update_timestamp_for_pubkey
enum Key {
    ContactInfo(Pubkey),
    Vote(Pubkey),
    LeaderId(Pubkey),
}

/// Value must correspond to Key
enum Value {
    ContactInfo(ContactInfo),
    /// TODO, Transactions need a height!!!
    Vote(Transaction, u64),
    /// My Id, My Current leader, version
    LeaderId(Pubkey, Pubkey, u64),
}

enum CrdtError {
    InsertFailed,
}

struct StoredValue {
    value: Value,
    /// local time when added
    local_timestamp: u64,
    /// local crdt version when added
    local_version: u64,
}

impl Key {
     fn pubkey(&self) -> Pubkey {
        match self {
            Key::ContactInfo(p) => p,
            Key::Vote(p) => p,
            Key::LeaderId(p) => p,
        }
    }  
}
 
impl Value {
    fn version(&self) -> u64 {
        match self {
            Value::ContactInfo(n) => n.version,
            Value::Vote(_, height) => height,
            Value::LeaderId(_, version) => version,
        }
    }
    fn id(&self) -> Key {
        match self {
            Value::ContactInfo(n) => Key::ContactInfo(n.id),
            Value::Vote(tx, height) => Key::Vote(tx.account_keys[0]),
            Value::LeaderId(id, _, version) => Key::LeaderId(id),
        }
    } 
}

pub struct Crdt {
    /// key value hashmap
    pub table: HashMap<Key, StoredValue>,

    /// the version of the `table`
    /// every change to `table` should increase this version number
    pub version: u64,

    /// The value of the remote `version` for ContactInfo's
    pub remote_versions: HashMap<Pubkey, u64>,
}

impl Crdt {
    fn insert(&mut self, value: Value, local_time: u64) -> Result<(), CrdtError> {
        let key = value.id();
        if self.table.get(&key).is_none() || (value.version() > self.table[&key].version()) {
            trace!("{}: insert value.id: {} version: {}", self.id, value.id, value.version);
            if self.table.get(&value.id).is_none() {
                inc_new_counter_info!("cluster_info-insert-new_entry", 1, 1);
            }

            let stored_value = StoredValue { value, local_time, local_version: self.version };
            let _ = self.table.insert(key, stored_value);
            self.version += 1;
            Ok(())
        } else {
            trace!(
                "{}: INSERT FAILED data: {} new.version: {} me.version: {}",
                self.id,
                value.id(),
                value.version(),
                self.table[&value.id()].version()
            );
            Err(CrdtError::InsertFailed)
        } 
    }

    fn update_timestamp(&mut self, id: Key) {
        //update the liveness table
        let now = timestamp();
        self.table.entry(id).map(|e| e.local_time = now);
    } 

    /// update all the key types
    pub fn update_timestamp_for_pubkey(&mut self, pubkey: Pubkey) {
        self.update_timestamp(NodeInfo(pubkey));
        self.update_timestamp(Vote(pubkey));
        self.update_timestamp(LeaderId(pubkey));
    }

    /// find all the keys that are older then min_ts
    pub fn find_old_keys(&self, min_ts: u64) -> Vec<Key> {
        self
            .table
            .iter()
            .filter_map(|(k, v)| {
                if v.local_timestamp < min_ts {
                    Some(k)
                } else {
                    None
                }
            }).collect()
    }

    pub fn remove(&mut self, keys: &[Key]) {
        for k in keys {
            self.table.remove(k);
        }
    }

   /// Get updated node since min_version up to a maximum of `max_number` of updates
   /// * min_version - return updates greater then min_version
   /// * max_number - max number of update
   /// * Returns (max version, updates)  
   /// * max version - the maximum version that is in the updates
   /// * updates - a vector of (Values, Value's remote update index) that have been changed.  Values
   /// remote update index is the last update index seen from the ContactInfo that is referenced by
   /// Value::id().pubkey().
   pub fn get_updates_since(&self, min_version: u64, max_number: usize) -> (u64, Vec<(Value, u64)>) {
        let mut items: Vec<_> = self
            .table
            .filter_map(|(k,x)|  {
               if k.pubkey() != Pubkey::default() && x.local_version > min_version {
                   let remote = *self.remote.get(&d.id().pubkey()).unwrap_or(&0)
                   Some((x, remote))
               } else {
                   None
               }
            })
            .collect();
        items.sort_by_key(|k| k.0.local_version);
        let updated_data: Vec<(Value, u64)> = out.into_iter().take(max).map(|x| (x.0.value, x.1)).collect();
        trace!("get updates since response {} {}", v, updated_data.len());
        (max_updated_node, updated_data)
    }
}
mod test {
    #[test]
    fn insert_test() {
        let mut d = NodeInfo::new_localhost(Keypair::new().pubkey());
        assert_eq!(d.version, 0);
        let mut cluster_info = ClusterInfo::new(d.clone()).unwrap();
        assert_eq!(cluster_info.table[&d.id].version, 0);
        assert!(!cluster_info.alive.contains_key(&d.id));

        d.version = 2;
        cluster_info.insert(&d);
        let liveness = cluster_info.alive[&d.id];
        assert_eq!(cluster_info.table[&d.id].version, 2);

        d.version = 1;
        cluster_info.insert(&d);
        assert_eq!(cluster_info.table[&d.id].version, 2);
        assert_eq!(liveness, cluster_info.alive[&d.id]);

        // Ensure liveness will be updated for version 3
        sleep(Duration::from_millis(1));

        d.version = 3;
        cluster_info.insert(&d);
        assert_eq!(cluster_info.table[&d.id].version, 3);
        assert!(liveness < cluster_info.alive[&d.id]);
    } 

    #[test]
    fn update_test() {
        let d1 = NodeInfo::new_localhost(Keypair::new().pubkey());
        let d2 = NodeInfo::new_localhost(Keypair::new().pubkey());
        let d3 = NodeInfo::new_localhost(Keypair::new().pubkey());
        let mut cluster_info = ClusterInfo::new(d1.clone()).expect("ClusterInfo::new");
        let (key, ix, ups) = cluster_info.get_updates_since(0, 1);
        assert_eq!(key, d1.id);
        assert_eq!(ix, 1);
        assert_eq!(ups.len(), 1);
        assert_eq!(sorted(&ups), sorted(&vec![(d1.clone(), 0)]));
        cluster_info.insert(&d2);
        let (key, ix, ups) = cluster_info.get_updates_since(0, 2);
        assert_eq!(key, d1.id);
        assert_eq!(ix, 2);
        assert_eq!(ups.len(), 2);
        assert_eq!(
            sorted(&ups),
            sorted(&vec![(d1.clone(), 0), (d2.clone(), 0)])
        );
        cluster_info.insert(&d3);
        let (key, ix, ups) = cluster_info.get_updates_since(0, 3);
        assert_eq!(key, d1.id);
        assert_eq!(ix, 3);
        assert_eq!(ups.len(), 3);
        assert_eq!(
            sorted(&ups),
            sorted(&vec![(d1.clone(), 0), (d2.clone(), 0), (d3.clone(), 0)])
        );
        let mut cluster_info2 = ClusterInfo::new(d2.clone()).expect("ClusterInfo::new");
        cluster_info2.apply_updates(key, ix, &ups);
        assert_eq!(cluster_info2.table.values().len(), 3);
        assert_eq!(
            sorted(
                &cluster_info2
                    .table
                    .values()
                    .map(|x| (x.clone(), 0))
                    .collect()
            ),
            sorted(
                &cluster_info
                    .table
                    .values()
                    .map(|x| (x.clone(), 0))
                    .collect()
            )
        );
        let d4 = NodeInfo::new_entry_point(&socketaddr!("127.0.0.4:1234"));
        cluster_info.insert(&d4);
        let (_key, ix, ups) = cluster_info.get_updates_since(0, 3);
        assert_eq!(
            sorted(&ups),
            sorted(&vec![(d2.clone(), 0), (d1.clone(), 0), (d3.clone(), 0)])
        );
        assert_eq!(ix, 3);

        let (_key, ix, ups) = cluster_info.get_updates_since(0, 2);
        assert_eq!(
            sorted(&ups),
            sorted(&vec![(d2.clone(), 0), (d1.clone(), 0)])
        );
        assert_eq!(ix, 2);

        let (_key, ix, ups) = cluster_info.get_updates_since(0, 1);
        assert_eq!(sorted(&ups), sorted(&vec![(d1.clone(), 0)]));
        assert_eq!(ix, 1);

        let (_key, ix, ups) = cluster_info.get_updates_since(1, 3);
        assert_eq!(ups.len(), 2);
        assert_eq!(ix, 3);
        assert_eq!(sorted(&ups), sorted(&vec![(d2, 0), (d3, 0)]));
        let (_key, ix, ups) = cluster_info.get_updates_since(3, 3);
        assert_eq!(ups.len(), 0);
        assert_eq!(ix, 0);
    } 

    #[test]
    fn purge_test() {
        logger::setup();
        let me = NodeInfo::new_with_socketaddr(&socketaddr!("127.0.0.1:1234"));
        let mut cluster_info = ClusterInfo::new(me.clone()).expect("ClusterInfo::new");
        let nxt = NodeInfo::new_with_socketaddr(&socketaddr!("127.0.0.2:1234"));
        assert_ne!(me.id, nxt.id);
        cluster_info.set_leader(me.id);
        cluster_info.insert(&nxt);
        let rv = cluster_info.gossip_request().unwrap();
        assert_eq!(rv.0, nxt.contact_info.ncp);
        let now = cluster_info.alive[&nxt.id];
        cluster_info.purge(now);
        let rv = cluster_info.gossip_request().unwrap();
        assert_eq!(rv.0, nxt.contact_info.ncp);

        cluster_info.purge(now + GOSSIP_PURGE_MILLIS);
        let rv = cluster_info.gossip_request().unwrap();
        assert_eq!(rv.0, nxt.contact_info.ncp);

        cluster_info.purge(now + GOSSIP_PURGE_MILLIS + 1);
        let rv = cluster_info.gossip_request().unwrap();
        assert_eq!(rv.0, nxt.contact_info.ncp);

        let mut nxt2 = NodeInfo::new_with_socketaddr(&socketaddr!("127.0.0.2:1234"));
        assert_ne!(me.id, nxt2.id);
        assert_ne!(nxt.id, nxt2.id);
        cluster_info.insert(&nxt2);
        while now == cluster_info.alive[&nxt2.id] {
            sleep(Duration::from_millis(GOSSIP_SLEEP_MILLIS));
            nxt2.version += 1;
            cluster_info.insert(&nxt2);
        }
        let len = cluster_info.table.len() as u64;
        assert!((MIN_TABLE_SIZE as u64) < len);
        cluster_info.purge(now + GOSSIP_PURGE_MILLIS);
        assert_eq!(len as usize, cluster_info.table.len());
        trace!("purging");
        cluster_info.purge(now + GOSSIP_PURGE_MILLIS + 1);
        assert_eq!(len as usize - 1, cluster_info.table.len());
        let rv = cluster_info.gossip_request().unwrap();
        assert_eq!(rv.0, nxt.contact_info.ncp);
    } 
}
