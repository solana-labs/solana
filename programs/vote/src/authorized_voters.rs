use log::*;
use serde_derive::{Deserialize, Serialize};
use solana_sdk::{clock::Epoch, clock::Slot, pubkey::Pubkey};
use std::collections::BTreeMap;

#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Eq, Clone, AbiExample)]
pub struct AuthorizedVoters {
    authorized_voters: BTreeMap<Epoch, Pubkey>,
}

impl AuthorizedVoters {
    pub fn new(epoch: Epoch, pubkey: Pubkey) -> Self {
        let mut authorized_voters = BTreeMap::new();
        authorized_voters.insert(epoch, pubkey);
        Self { authorized_voters }
    }

    pub fn get_authorized_voter(&self, epoch: Epoch) -> Option<Pubkey> {
        self.get_or_calculate_authorized_voter_for_epoch(epoch)
            .map(|(pubkey, _)| pubkey)
    }

    pub fn get_and_cache_authorized_voter_for_epoch(&mut self, epoch: Epoch) -> Option<Pubkey> {
        let res = self.get_or_calculate_authorized_voter_for_epoch(epoch);

        res.map(|(pubkey, existed)| {
            if !existed {
                self.authorized_voters.insert(epoch, pubkey);
            }
            pubkey
        })
    }

    pub fn insert(&mut self, epoch: Epoch, authorized_voter: Pubkey) {
        self.authorized_voters.insert(epoch, authorized_voter);
    }

    pub fn purge_authorized_voters(&mut self, current_epoch: Epoch) -> bool {
        // Iterate through the keys in order, filtering out the ones
        // less than the current epoch
        let expired_keys: Vec<_> = self
            .authorized_voters
            .range(0..current_epoch)
            .map(|(authorized_epoch, _)| *authorized_epoch)
            .collect();

        for key in expired_keys {
            self.authorized_voters.remove(&key);
        }

        // Have to uphold this invariant b/c this is
        // 1) The check for whether the vote state is initialized
        // 2) How future authorized voters for uninitialized epochs are set
        //    by this function
        assert!(!self.authorized_voters.is_empty());
        true
    }

    pub fn is_empty(&self) -> bool {
        self.authorized_voters.is_empty()
    }

    pub fn first(&self) -> Option<(&u64, &Pubkey)> {
        self.authorized_voters.iter().next()
    }

    pub fn last(&self) -> Option<(&u64, &Pubkey)> {
        self.authorized_voters.iter().next_back()
    }

    pub fn len(&self) -> usize {
        self.authorized_voters.len()
    }

    pub fn contains(&self, epoch: Epoch) -> bool {
        self.authorized_voters.get(&epoch).is_some()
    }

    pub fn iter(&self) -> std::collections::btree_map::Iter<Epoch, Pubkey> {
        self.authorized_voters.iter()
    }


    pub fn get_authorized_slot_voter(&self, epoch: Epoch, slot: Slot) -> &solana_sdk::pubkey::Pubkey {
        self.get_or_calculate_authorized_voter_for_slot(epoch, slot)
    }
    

    // Returns the authorized voter at the given epoch if the epoch is >= the
    // current epoch, and a bool indicating whether the entry for this epoch
    // exists in the self.authorized_voter map
    fn get_or_calculate_authorized_voter_for_epoch(&self, epoch: Epoch) -> Option<(Pubkey, bool)> {
        let res = self.authorized_voters.get(&epoch);
        if res.is_none() {
            // If no authorized voter has been set yet for this epoch,
            // this must mean the authorized voter remains unchanged
            // from the latest epoch before this one
            let res = self.authorized_voters.range(0..epoch).next_back();

            if res.is_none() {
                warn!(
                    "Tried to query for the authorized voter of an epoch earlier
                    than the current epoch. Earlier epochs have been purged"
                );
            }

            res.map(|(_, pubkey)| (*pubkey, false))
        } else {
            res.map(|pubkey| (*pubkey, true))
        }
    }






    // Returns the authorized voter at the given epoch if the epoch is >= the
    // current epoch, and a bool indicating whether the entry for this epoch
    // exists in the self.authorized_voter map
    fn get_or_calculate_authorized_voter_for_slot(&self, epoch: Epoch, slot: Slot) -> &solana_sdk::pubkey::Pubkey {
        let res = self.authorized_voters.get(&epoch);



            let _strt = 0;
            let vote_count = res.iter().count() as u64;
            if vote_count > 1 {
            let _strt = slot % 10;
                 if _strt > vote_count {
                 let _strt = _strt/2;
                 }
                 if _strt > vote_count {
                 let _strt = _strt/2;
                 }
                 if _strt > vote_count {
                 let _strt = _strt/2;
                 }
            }
            if vote_count > 9 {
            let _strt = (slot % 100)/5;
                 if _strt > vote_count {
                 let _strt = _strt/2;
                 }
                 if _strt > vote_count {
                 let _strt = _strt/2;
                 }
                 if _strt > vote_count {
                 let _strt = _strt/2;
                 }
                 if _strt > vote_count {
                 let _strt = _strt/2;
                 }
            }
            if vote_count > 99 {
            let _strt = (slot % 1000)/5;
                 if _strt > vote_count {
                 let _strt = _strt/2;
                 }
                 if _strt > vote_count {
                 let _strt = _strt/2;
                 }
                 if _strt > vote_count {
                 let _strt = _strt/2;
                 }
                 if _strt > vote_count {
                 let _strt = _strt/2;
                 }
                 if _strt > vote_count {
                 let _strt = _strt/2;
                 }
            }
            let _strt = _strt as usize;


          let mut authorized_voters: Vec<_> = res.into_iter().collect();
           authorized_voters.sort();
           let auth_voter = authorized_voters[_strt];


auth_voter

    }



}
