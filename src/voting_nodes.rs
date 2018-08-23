//! The `voting_nodes` maintains a map of peer nodes that are actively
//! voting on the transactions

use byteorder::{LittleEndian, ReadBytesExt};
use counter::Counter;
use crdt::{Crdt, NodeInfo};
use hash::Hash;
use log::Level;
use signature::Pubkey;
use std::collections::HashMap;
use std::io::Cursor;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use std::thread::{sleep, Builder};
use std::time::Duration;
use timing::timestamp;
use transaction::Vote;

const VOTING_NODES_PURGE_SLEEP_MILLIS: u64 = 100;
const VOTING_NODES_PURGE_MILLIS: u64 = 15000;

pub struct VotingNodes {
    pub nodes: Arc<RwLock<HashMap<Pubkey, u64>>>,
}

fn make_debug_id(key: &Pubkey) -> u64 {
    let buf: &[u8] = &key.as_ref();
    let mut rdr = Cursor::new(&buf[..8]);
    rdr.read_u64::<LittleEndian>()
        .expect("rdr.read_u64 in fn debug_id")
}

impl VotingNodes {
    pub fn new(crdt: Arc<RwLock<Crdt>>, exit: Arc<AtomicBool>) -> Self {
        let nodes_arc = Arc::new(RwLock::new(HashMap::new()));
        let me = VotingNodes {
            nodes: nodes_arc.clone(),
        };

        Builder::new()
            .name("solana-voting-nodes".to_string())
            .spawn(move || loop {
                if exit.load(Ordering::Relaxed) {
                    return;
                }
                VotingNodes::purge(&crdt, &nodes_arc, timestamp());
                sleep(Duration::from_millis(VOTING_NODES_PURGE_SLEEP_MILLIS));
            })
            .unwrap();

        me
    }

    fn purge(crdt: &Arc<RwLock<Crdt>>, nodes_arc: &Arc<RwLock<HashMap<Pubkey, u64>>>, now: u64) {
        nodes_arc.write().unwrap().retain(|key, last_update| {
            if (now - *last_update) > VOTING_NODES_PURGE_MILLIS {
                crdt.write().unwrap().purge_node(key);
                false
            } else {
                true
            }
        });
    }

    fn update_liveness(&mut self, debug_id: u64, id: Pubkey) {
        //update the liveness table
        let now = timestamp();
        trace!(
            "{:x} updating liveness {:x} to {}",
            debug_id,
            make_debug_id(&id),
            now
        );
        *self.nodes.write().unwrap().entry(id).or_insert(now) = now;
    }

    pub fn insert_vote(
        &mut self,
        crdt: &Arc<RwLock<Crdt>>,
        pubkey: &Pubkey,
        v: &Vote,
        last_id: Hash,
    ) {
        let mut wcrdt = crdt.write().unwrap();

        if wcrdt.table.get(pubkey).is_none() {
            warn!(
                "{:x}: VOTE for unknown id: {:x}",
                wcrdt.debug_id(),
                make_debug_id(pubkey)
            );
            return;
        }
        if v.contact_info_version > wcrdt.table[pubkey].contact_info.version {
            warn!(
                "{:x}: VOTE for new address version from: {:x} ours: {} vote: {:?}",
                wcrdt.debug_id(),
                make_debug_id(pubkey),
                wcrdt.table[pubkey].contact_info.version,
                v,
            );
            return;
        }
        if *pubkey == wcrdt.my_data().leader_id {
            info!(
                "{:x}: LEADER_VOTED! {:x}",
                wcrdt.debug_id(),
                make_debug_id(&pubkey)
            );
            inc_new_counter_info!("crdt-insert_vote-leader_voted", 1);
        }

        if v.version <= wcrdt.table[pubkey].version {
            debug!(
                "{:x}: VOTE for old version: {:x}",
                wcrdt.debug_id(),
                make_debug_id(&pubkey)
            );
            self.update_liveness(wcrdt.debug_id(), *pubkey);
            return;
        }
        let mut data = wcrdt.table[pubkey].clone();
        data.version = v.version;
        data.ledger_state.last_id = last_id;

        debug!(
            "{:x}: INSERTING VOTE! for {:x}",
            wcrdt.debug_id(),
            data.debug_id()
        );
        self.update_liveness(wcrdt.debug_id(), data.id);
        wcrdt.insert(&data);
    }

    /// compute broadcast table
    /// # Remarks
    pub fn compute_broadcast_table(&self, crdt: &Arc<RwLock<Crdt>>) -> Vec<NodeInfo> {
        let rcrdt = crdt.read().unwrap();
        let rnodes = self.nodes.read().unwrap();
        let live: Vec<_> = rnodes.iter().collect();
        //thread_rng().shuffle(&mut live);
        let me = &rcrdt.table[&rcrdt.me];
        let cloned_table: Vec<NodeInfo> = live
            .iter()
            .map(|x| &rcrdt.table[x.0])
            .filter(|v| {
                if me.id == v.id {
                    //filter myself
                    false
                } else if !(Crdt::is_valid_address(v.contact_info.tvu)) {
                    trace!(
                        "{:x}:broadcast skip not listening {:x}",
                        me.debug_id(),
                        v.debug_id()
                    );
                    false
                } else {
                    trace!(
                        "{:x}:broadcast node {:x} {}",
                        me.debug_id(),
                        v.debug_id(),
                        v.contact_info.tvu
                    );
                    true
                }
            })
            .cloned()
            .collect();
        cloned_table
    }

    pub fn insert_votes(&mut self, crdt: &Arc<RwLock<Crdt>>, votes: &[(Pubkey, Vote, Hash)]) {
        inc_new_counter_info!("crdt-vote-count", votes.len());
        for v in votes {
            self.insert_vote(crdt, &v.0, &v.1, v.2);
        }
    }
}

#[cfg(test)]
pub mod tests {
    use crdt::{Crdt, NodeInfo};
    use hash::Hash;
    use logger;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, RwLock};
    use transaction::Vote;
    use voting_nodes::{VotingNodes, VOTING_NODES_PURGE_MILLIS};

    #[test]
    fn test_insert_vote() {
        let d = NodeInfo::new_leader(&"127.0.0.1:1234".parse().unwrap());
        assert_eq!(d.version, 0);
        let crdt = Crdt::new(d.clone()).unwrap();
        let crdt_arc = Arc::new(RwLock::new(crdt));
        {
            let rcrdt = crdt_arc.read().unwrap();
            assert_eq!(rcrdt.table[&d.id].version, 0);
        }
        let exit = Arc::new(AtomicBool::new(false));
        let mut voting_nodes = VotingNodes::new(crdt_arc.clone(), exit.clone());
        let vote_same_version = Vote {
            version: d.version,
            contact_info_version: 0,
        };
        voting_nodes.insert_vote(
            &crdt_arc.clone(),
            &d.id,
            &vote_same_version,
            Hash::default(),
        );
        {
            let rcrdt = crdt_arc.read().unwrap();
            assert_eq!(rcrdt.table[&d.id].version, 0);
        }
        let vote_new_version_new_addrs = Vote {
            version: d.version + 1,
            contact_info_version: 1,
        };
        voting_nodes.insert_vote(
            &crdt_arc.clone(),
            &d.id,
            &vote_new_version_new_addrs,
            Hash::default(),
        );
        //should be dropped since the address is newer then we know
        {
            let rcrdt = crdt_arc.read().unwrap();
            assert_eq!(rcrdt.table[&d.id].version, 0);
        }

        let vote_new_version_old_addrs = Vote {
            version: d.version + 1,
            contact_info_version: 0,
        };
        voting_nodes.insert_vote(
            &crdt_arc.clone(),
            &d.id,
            &vote_new_version_old_addrs,
            Hash::default(),
        );
        //should be accepted, since the update is for the same address field as the one we know
        {
            let rcrdt = crdt_arc.read().unwrap();
            assert_eq!(rcrdt.table[&d.id].version, 1);
        }
        exit.store(true, Ordering::Relaxed);
    }

    #[test]
    fn purge_test() {
        logger::setup();
        let me = NodeInfo::new_leader(&"127.0.0.1:1234".parse().unwrap());
        let crdt = Crdt::new(me.clone()).expect("Crdt::new");
        let crdt_arc = Arc::new(RwLock::new(crdt));
        let exit = Arc::new(AtomicBool::new(false));
        let mut voting_nodes = VotingNodes::new(crdt_arc.clone(), exit.clone());

        let nxt = NodeInfo::new_leader(&"127.0.0.2:1234".parse().unwrap());
        assert_ne!(me.id, nxt.id);
        {
            let mut wcrdt = crdt_arc.write().unwrap();
            wcrdt.set_leader(me.id);
            wcrdt.insert(&nxt);
        }
        let vote_nxt = Vote {
            version: nxt.version + 1,
            contact_info_version: 0,
        };

        voting_nodes.insert_vote(&crdt_arc.clone(), &nxt.id, &vote_nxt, Hash::default());
        {
            let rcrdt = crdt_arc.read().unwrap();
            assert_eq!(rcrdt.table.len(), 2);
        }

        let now = {
            let rnodes = voting_nodes.nodes.read().unwrap();
            rnodes[&nxt.id]
        };

        VotingNodes::purge(&crdt_arc, &voting_nodes.nodes, now);
        {
            let rcrdt = crdt_arc.read().unwrap();
            assert_eq!(rcrdt.table.len(), 2);
        }

        VotingNodes::purge(
            &crdt_arc,
            &voting_nodes.nodes,
            now + VOTING_NODES_PURGE_MILLIS,
        );
        {
            let rcrdt = crdt_arc.read().unwrap();
            assert_eq!(rcrdt.table.len(), 2);
        }

        VotingNodes::purge(
            &crdt_arc,
            &voting_nodes.nodes,
            now + VOTING_NODES_PURGE_MILLIS + 1,
        );
        {
            let rcrdt = crdt_arc.read().unwrap();
            assert_eq!(rcrdt.table.len(), 2);
        }

        let nxt2 = NodeInfo::new_leader(&"127.0.0.2:1234".parse().unwrap());
        assert_ne!(me.id, nxt2.id);
        assert_ne!(nxt.id, nxt2.id);
        {
            let mut wcrdt = crdt_arc.write().unwrap();
            wcrdt.insert(&nxt2);
        }

        let vote_nxt2 = Vote {
            version: nxt2.version + 1,
            contact_info_version: 0,
        };

        voting_nodes.insert_vote(&crdt_arc.clone(), &nxt2.id, &vote_nxt2, Hash::default());
        {
            let rcrdt = crdt_arc.read().unwrap();
            assert_eq!(rcrdt.table.len(), 3);
        }

        let now = {
            let rnodes = voting_nodes.nodes.read().unwrap();
            rnodes[&nxt2.id]
        };

        VotingNodes::purge(
            &crdt_arc,
            &voting_nodes.nodes,
            now + VOTING_NODES_PURGE_MILLIS,
        );
        {
            let rcrdt = crdt_arc.read().unwrap();
            assert_eq!(rcrdt.table.len(), 3);
        }
        trace!("purging");
        VotingNodes::purge(
            &crdt_arc,
            &voting_nodes.nodes,
            now + VOTING_NODES_PURGE_MILLIS + 1,
        );
        {
            let rcrdt = crdt_arc.read().unwrap();
            assert_eq!(rcrdt.table.len(), 2);
        }
    }
}
