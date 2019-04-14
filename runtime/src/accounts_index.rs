use hashbrown::{HashMap, HashSet};
use log::*;
use solana_sdk::pubkey::Pubkey;

pub type Fork = u64;

#[derive(Default)]
pub struct AccountsIndex<T> {
    account_maps: HashMap<Pubkey, Vec<(Fork, T)>>,
    roots: HashSet<Fork>,
    //This value that needs to be stored to recover the index from AppendVec
    last_root: Fork,
}

impl<T: Clone> AccountsIndex<T> {
    /// Get an account
    /// The latest account that appears in `ancestors` or `roots` is returned.
    pub fn get(&self, pubkey: &Pubkey, ancestors: &HashMap<Fork, usize>) -> Option<&T> {
        let list = self.account_maps.get(pubkey)?;
        let mut max = 0;
        let mut rv = None;
        for e in list.iter().rev() {
            if e.0 >= max && (ancestors.get(&e.0).is_some() || self.is_root(e.0)) {
                trace!("GET {} {:?}", e.0, ancestors);
                rv = Some(&e.1);
                max = e.0;
            }
        }
        rv
    }

    /// Insert a new fork.  
    /// @retval - The return value contains any squashed accounts that can freed from storage.
    pub fn insert(&mut self, fork: Fork, pubkey: &Pubkey, account_info: T) -> Vec<(Fork, T)> {
        let mut rv = vec![];
        let mut fork_vec: Vec<(Fork, T)> = vec![];
        {
            let entry = self.account_maps.entry(*pubkey).or_insert(vec![]);
            std::mem::swap(entry, &mut fork_vec);
        };

        // filter out old entries
        rv.extend(fork_vec.iter().filter(|(f, _)| *f == fork).cloned());
        fork_vec.retain(|(f, _)| *f != fork);

        // add the new entry
        fork_vec.push((fork, account_info));

        // do some cleanup
        let mut max = 0;
        for (fork, _) in fork_vec.iter() {
            if *fork >= max && self.is_root(*fork) {
                max = *fork;
            }
        }
        rv.extend(
            fork_vec
                .iter()
                .filter(|(fork, _)| !self.is_valid(max, *fork))
                .cloned(),
        );
        fork_vec.retain(|(fork, _)| self.is_valid(max, *fork));
        {
            let entry = self.account_maps.entry(*pubkey).or_insert(vec![]);
            std::mem::swap(entry, &mut fork_vec);
        };
        rv
    }
    fn is_valid(&self, max: u64, fork: u64) -> bool {
        fork >= max && !self.is_purged(fork)
    }
    fn is_purged(&self, fork: Fork) -> bool {
        !self.is_root(fork) && fork < self.last_root
    }
    pub fn is_root(&self, fork: Fork) -> bool {
        self.roots.contains(&fork)
    }
    pub fn add_root(&mut self, fork: Fork) {
        if self.roots.contains(&fork) {
            trace!("duplicate add root {} {}", fork, self.last_root);
            return;
        }
        assert!(self.last_root == 0 || fork > self.last_root);
        self.last_root = fork;
        self.roots.insert(fork);
    }
    /// Remove the fork when the storage for the fork is freed
    /// Accounts no longer reference this fork.
    pub fn cleanup_dead_fork(&mut self, fork: Fork) {
        self.roots.remove(&fork);
    }
}
