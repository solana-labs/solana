//! Crds Gossip Push overlay
//! This module is used to propagate recently created CrdsValues across the network
//! Eager push strategy is based on Plumtree
//! http://asc.di.fct.unl.pt/~jleitao/pdf/srds07-leitao.pdf
//!
//! Main differences are:
//! 1. There is no `max hop`.  Messages are signed with a local wallclock.  If they are outside of
//!    the local nodes wallclock window they are drooped silently.
//! 2. The prune set is stored in a Bloom filter.

use crate::bloom::Bloom;
use crate::contact_info::ContactInfo;
use crate::crds::{Crds, VersionedCrdsValue};
use crate::crds_gossip::CRDS_GOSSIP_BLOOM_SIZE;
use crate::crds_gossip_error::CrdsGossipError;
use crate::crds_value::{CrdsValue, CrdsValueLabel};
use crate::packet::BLOB_DATA_SIZE;
use bincode::serialized_size;
use hashbrown::HashMap;
use indexmap::map::IndexMap;
use rand;
use rand::seq::SliceRandom;
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use std::cmp;

pub const CRDS_GOSSIP_NUM_ACTIVE: usize = 30;
pub const CRDS_GOSSIP_PUSH_FANOUT: usize = 6;
pub const CRDS_GOSSIP_PUSH_MSG_TIMEOUT_MS: u64 = 5000;
pub const CRDS_GOSSIP_PRUNE_MSG_TIMEOUT_MS: u64 = 500;

pub struct CrdsGossipPush {
    /// max bytes per message
    pub max_bytes: usize,
    /// active set of validators for push
    active_set: IndexMap<Pubkey, Bloom<Pubkey>>,
    /// push message queue
    push_messages: HashMap<CrdsValueLabel, Hash>,
    pushed_once: HashMap<Hash, u64>,
    pub num_active: usize,
    pub push_fanout: usize,
    pub msg_timeout: u64,
    pub prune_timeout: u64,
}

impl Default for CrdsGossipPush {
    fn default() -> Self {
        Self {
            max_bytes: BLOB_DATA_SIZE,
            active_set: IndexMap::new(),
            push_messages: HashMap::new(),
            pushed_once: HashMap::new(),
            num_active: CRDS_GOSSIP_NUM_ACTIVE,
            push_fanout: CRDS_GOSSIP_PUSH_FANOUT,
            msg_timeout: CRDS_GOSSIP_PUSH_MSG_TIMEOUT_MS,
            prune_timeout: CRDS_GOSSIP_PRUNE_MSG_TIMEOUT_MS,
        }
    }
}
impl CrdsGossipPush {
    pub fn num_pending(&self) -> usize {
        self.push_messages.len()
    }
    /// process a push message to the network
    pub fn process_push_message(
        &mut self,
        crds: &mut Crds,
        value: CrdsValue,
        now: u64,
    ) -> Result<Option<VersionedCrdsValue>, CrdsGossipError> {
        if now > value.wallclock() + self.msg_timeout {
            return Err(CrdsGossipError::PushMessageTimeout);
        }
        if now + self.msg_timeout < value.wallclock() {
            return Err(CrdsGossipError::PushMessageTimeout);
        }
        let label = value.label();

        let new_value = crds.new_versioned(now, value);
        let value_hash = new_value.value_hash;
        if self.pushed_once.get(&value_hash).is_some() {
            return Err(CrdsGossipError::PushMessagePrune);
        }
        let old = crds.insert_versioned(new_value);
        if old.is_err() {
            return Err(CrdsGossipError::PushMessageOldVersion);
        }
        self.push_messages.insert(label, value_hash);
        self.pushed_once.insert(value_hash, now);
        Ok(old.ok().and_then(|opt| opt))
    }

    /// New push message to broadcast to peers.
    /// Returns a list of Pubkeys for the selected peers and a list of values to send to all the
    /// peers.
    /// The list of push messages is created such that all the randomly selected peers have not
    /// pruned the source addresses.
    pub fn new_push_messages(&mut self, crds: &Crds, now: u64) -> (Vec<Pubkey>, Vec<CrdsValue>) {
        let max = self.active_set.len();
        let mut nodes: Vec<_> = (0..max).collect();
        nodes.shuffle(&mut rand::thread_rng());
        let peers: Vec<Pubkey> = nodes
            .into_iter()
            .filter_map(|n| self.active_set.get_index(n))
            .take(self.push_fanout)
            .map(|n| *n.0)
            .collect();
        let mut total_bytes: usize = 0;
        let mut values = vec![];
        for (label, hash) in &self.push_messages {
            let mut failed = false;
            for p in &peers {
                let filter = self.active_set.get_mut(p);
                failed |= filter.is_none() || filter.unwrap().contains(&label.pubkey());
            }
            if failed {
                continue;
            }
            let res = crds.lookup_versioned(label);
            if res.is_none() {
                continue;
            }
            let version = res.unwrap();
            if version.value_hash != *hash {
                continue;
            }
            let value = &version.value;
            if value.wallclock() > now || value.wallclock() + self.msg_timeout < now {
                continue;
            }
            total_bytes += serialized_size(value).unwrap() as usize;
            if total_bytes > self.max_bytes {
                break;
            }
            values.push(value.clone());
        }
        for v in &values {
            self.push_messages.remove(&v.label());
        }
        (peers, values)
    }

    /// add the `from` to the peer's filter of nodes
    pub fn process_prune_msg(&mut self, peer: Pubkey, origins: &[Pubkey]) {
        for origin in origins {
            if let Some(p) = self.active_set.get_mut(&peer) {
                p.add(origin)
            }
        }
    }

    fn compute_need(num_active: usize, active_set_len: usize, ratio: usize) -> usize {
        let num = active_set_len / ratio;
        cmp::min(num_active, (num_active - active_set_len) + num)
    }

    /// refresh the push active set
    /// * ratio - active_set.len()/ratio is the number of actives to rotate
    pub fn refresh_push_active_set(
        &mut self,
        crds: &Crds,
        self_id: Pubkey,
        network_size: usize,
        ratio: usize,
    ) {
        let need = Self::compute_need(self.num_active, self.active_set.len(), ratio);
        let mut new_items = HashMap::new();
        let mut ixs: Vec<_> = (0..crds.table.len()).collect();
        ixs.shuffle(&mut rand::thread_rng());

        for ix in ixs {
            let item = crds.table.get_index(ix);
            if item.is_none() {
                continue;
            }
            let val = item.unwrap();
            if val.0.pubkey() == self_id {
                continue;
            }
            if self.active_set.get(&val.0.pubkey()).is_some() {
                continue;
            }
            if new_items.get(&val.0.pubkey()).is_some() {
                continue;
            }
            if let Some(contact) = val.1.value.contact_info() {
                if !ContactInfo::is_valid_address(&contact.gossip) {
                    continue;
                }
            }
            let size = cmp::max(CRDS_GOSSIP_BLOOM_SIZE, network_size);
            let bloom = Bloom::random(size, 0.1, 1024 * 8 * 4);
            new_items.insert(val.0.pubkey(), bloom);
            if new_items.len() == need {
                break;
            }
        }
        let mut keys: Vec<Pubkey> = self.active_set.keys().cloned().collect();
        keys.shuffle(&mut rand::thread_rng());
        let num = keys.len() / ratio;
        for k in &keys[..num] {
            self.active_set.remove(k);
        }
        for (k, v) in new_items {
            self.active_set.insert(k, v);
        }
    }

    /// purge old pending push messages
    pub fn purge_old_pending_push_messages(&mut self, crds: &Crds, min_time: u64) {
        let old_msgs: Vec<CrdsValueLabel> = self
            .push_messages
            .iter()
            .filter_map(|(k, hash)| {
                if let Some(versioned) = crds.lookup_versioned(k) {
                    if versioned.value.wallclock() < min_time || versioned.value_hash != *hash {
                        Some(k)
                    } else {
                        None
                    }
                } else {
                    Some(k)
                }
            })
            .cloned()
            .collect();
        for k in old_msgs {
            self.push_messages.remove(&k);
        }
    }
    /// purge old pushed_once messages
    pub fn purge_old_pushed_once_messages(&mut self, min_time: u64) {
        let old_msgs: Vec<Hash> = self
            .pushed_once
            .iter()
            .filter_map(|(k, v)| if *v < min_time { Some(k) } else { None })
            .cloned()
            .collect();
        for k in old_msgs {
            self.pushed_once.remove(&k);
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::contact_info::ContactInfo;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    #[test]
    fn test_process_push() {
        let mut crds = Crds::default();
        let mut push = CrdsGossipPush::default();
        let value = CrdsValue::ContactInfo(ContactInfo::new_localhost(Keypair::new().pubkey(), 0));
        let label = value.label();
        // push a new message
        assert_eq!(
            push.process_push_message(&mut crds, value.clone(), 0),
            Ok(None)
        );
        assert_eq!(crds.lookup(&label), Some(&value));

        // push it again
        assert_eq!(
            push.process_push_message(&mut crds, value.clone(), 0),
            Err(CrdsGossipError::PushMessagePrune)
        );
    }
    #[test]
    fn test_process_push_old_version() {
        let mut crds = Crds::default();
        let mut push = CrdsGossipPush::default();
        let mut ci = ContactInfo::new_localhost(Keypair::new().pubkey(), 0);
        ci.wallclock = 1;
        let value = CrdsValue::ContactInfo(ci.clone());

        // push a new message
        assert_eq!(push.process_push_message(&mut crds, value, 0), Ok(None));

        // push an old version
        ci.wallclock = 0;
        let value = CrdsValue::ContactInfo(ci.clone());
        assert_eq!(
            push.process_push_message(&mut crds, value, 0),
            Err(CrdsGossipError::PushMessageOldVersion)
        );
    }
    #[test]
    fn test_process_push_timeout() {
        let mut crds = Crds::default();
        let mut push = CrdsGossipPush::default();
        let timeout = push.msg_timeout;
        let mut ci = ContactInfo::new_localhost(Keypair::new().pubkey(), 0);

        // push a version to far in the future
        ci.wallclock = timeout + 1;
        let value = CrdsValue::ContactInfo(ci.clone());
        assert_eq!(
            push.process_push_message(&mut crds, value, 0),
            Err(CrdsGossipError::PushMessageTimeout)
        );

        // push a version to far in the past
        ci.wallclock = 0;
        let value = CrdsValue::ContactInfo(ci.clone());
        assert_eq!(
            push.process_push_message(&mut crds, value, timeout + 1),
            Err(CrdsGossipError::PushMessageTimeout)
        );
    }
    #[test]
    fn test_process_push_update() {
        let mut crds = Crds::default();
        let mut push = CrdsGossipPush::default();
        let mut ci = ContactInfo::new_localhost(Keypair::new().pubkey(), 0);
        ci.wallclock = 0;
        let value_old = CrdsValue::ContactInfo(ci.clone());

        // push a new message
        assert_eq!(
            push.process_push_message(&mut crds, value_old.clone(), 0),
            Ok(None)
        );

        // push an old version
        ci.wallclock = 1;
        let value = CrdsValue::ContactInfo(ci.clone());
        assert_eq!(
            push.process_push_message(&mut crds, value, 0)
                .unwrap()
                .unwrap()
                .value,
            value_old
        );
    }
    #[test]
    fn test_compute_need() {
        assert_eq!(CrdsGossipPush::compute_need(30, 0, 10), 30);
        assert_eq!(CrdsGossipPush::compute_need(30, 1, 10), 29);
        assert_eq!(CrdsGossipPush::compute_need(30, 30, 10), 3);
        assert_eq!(CrdsGossipPush::compute_need(30, 29, 10), 3);
    }
    #[test]
    fn test_refresh_active_set() {
        solana_logger::setup();
        let mut crds = Crds::default();
        let mut push = CrdsGossipPush::default();
        let value1 = CrdsValue::ContactInfo(ContactInfo::new_localhost(Keypair::new().pubkey(), 0));

        assert_eq!(crds.insert(value1.clone(), 0), Ok(None));
        push.refresh_push_active_set(&crds, Pubkey::default(), 1, 1);

        assert!(push.active_set.get(&value1.label().pubkey()).is_some());
        let value2 = CrdsValue::ContactInfo(ContactInfo::new_localhost(Keypair::new().pubkey(), 0));
        assert!(push.active_set.get(&value2.label().pubkey()).is_none());
        assert_eq!(crds.insert(value2.clone(), 0), Ok(None));
        for _ in 0..30 {
            push.refresh_push_active_set(&crds, Pubkey::default(), 1, 1);
            if push.active_set.get(&value2.label().pubkey()).is_some() {
                break;
            }
        }
        assert!(push.active_set.get(&value2.label().pubkey()).is_some());

        for _ in 0..push.num_active {
            let value2 =
                CrdsValue::ContactInfo(ContactInfo::new_localhost(Keypair::new().pubkey(), 0));
            assert_eq!(crds.insert(value2.clone(), 0), Ok(None));
        }
        push.refresh_push_active_set(&crds, Pubkey::default(), 1, 1);
        assert_eq!(push.active_set.len(), push.num_active);
    }
    #[test]
    fn test_new_push_messages() {
        let mut crds = Crds::default();
        let mut push = CrdsGossipPush::default();
        let peer = CrdsValue::ContactInfo(ContactInfo::new_localhost(Keypair::new().pubkey(), 0));
        assert_eq!(crds.insert(peer.clone(), 0), Ok(None));
        push.refresh_push_active_set(&crds, Pubkey::default(), 1, 1);

        let new_msg =
            CrdsValue::ContactInfo(ContactInfo::new_localhost(Keypair::new().pubkey(), 0));
        assert_eq!(
            push.process_push_message(&mut crds, new_msg.clone(), 0),
            Ok(None)
        );
        assert_eq!(push.active_set.len(), 1);
        assert_eq!(
            push.new_push_messages(&crds, 0),
            (vec![peer.label().pubkey()], vec![new_msg])
        );
    }
    #[test]
    fn test_process_prune() {
        let mut crds = Crds::default();
        let mut push = CrdsGossipPush::default();
        let peer = CrdsValue::ContactInfo(ContactInfo::new_localhost(Keypair::new().pubkey(), 0));
        assert_eq!(crds.insert(peer.clone(), 0), Ok(None));
        push.refresh_push_active_set(&crds, Pubkey::default(), 1, 1);

        let new_msg =
            CrdsValue::ContactInfo(ContactInfo::new_localhost(Keypair::new().pubkey(), 0));
        assert_eq!(
            push.process_push_message(&mut crds, new_msg.clone(), 0),
            Ok(None)
        );
        push.process_prune_msg(peer.label().pubkey(), &[new_msg.label().pubkey()]);
        assert_eq!(
            push.new_push_messages(&crds, 0),
            (vec![peer.label().pubkey()], vec![])
        );
    }
    #[test]
    fn test_purge_old_pending_push_messages() {
        let mut crds = Crds::default();
        let mut push = CrdsGossipPush::default();
        let peer = CrdsValue::ContactInfo(ContactInfo::new_localhost(Keypair::new().pubkey(), 0));
        assert_eq!(crds.insert(peer.clone(), 0), Ok(None));
        push.refresh_push_active_set(&crds, Pubkey::default(), 1, 1);

        let mut ci = ContactInfo::new_localhost(Keypair::new().pubkey(), 0);
        ci.wallclock = 1;
        let new_msg = CrdsValue::ContactInfo(ci.clone());
        assert_eq!(
            push.process_push_message(&mut crds, new_msg.clone(), 1),
            Ok(None)
        );
        push.purge_old_pending_push_messages(&crds, 0);
        assert_eq!(
            push.new_push_messages(&crds, 0),
            (vec![peer.label().pubkey()], vec![])
        );
    }

    #[test]
    fn test_purge_old_pushed_once_messages() {
        let mut crds = Crds::default();
        let mut push = CrdsGossipPush::default();
        let mut ci = ContactInfo::new_localhost(Keypair::new().pubkey(), 0);
        ci.wallclock = 0;
        let value = CrdsValue::ContactInfo(ci.clone());
        let label = value.label();
        // push a new message
        assert_eq!(
            push.process_push_message(&mut crds, value.clone(), 0),
            Ok(None)
        );
        assert_eq!(crds.lookup(&label), Some(&value));

        // push it again
        assert_eq!(
            push.process_push_message(&mut crds, value.clone(), 0),
            Err(CrdsGossipError::PushMessagePrune)
        );

        // purge the old pushed
        push.purge_old_pushed_once_messages(1);

        // push it again
        assert_eq!(
            push.process_push_message(&mut crds, value.clone(), 0),
            Err(CrdsGossipError::PushMessageOldVersion)
        );
    }
}
