//! Fork Selection Simulation
//!
//! Description of the algorithm can be found in [docs/src/fork-selection.md](docs/src/fork-selection.md).
//!
//! A test library function exists for configuring networks.
//! ```
//!     /// * num_partitions - 1 to 100 partitions
//!     /// * fail_rate - 0 to 1.0 rate of packet receive failure
//!     /// * delay_count - number of forks to observe before voting
//!     /// * parasite_rate - number of parasite nodes that vote opposite the greedy choice
//!     fn test_with_partitions(num_partitions: usize, fail_rate: f64, delay_count: usize, parasite_rate: f64);
//! ```
//! Modify the test function
//! ```
//! #[test]
//! #[ignore]
//! fn test_all_partitions() {
//!     test_with_partitions(100, 0.0, 5, 0.25, false)
//! }
//! ```
//! Run with cargo
//!
//! ```
//! cargo test all_partitions --release -- --nocapture --ignored
//! ```
//!
//! The output will look like this
//! ```
//! time: 336, tip converged: 76, trunk id: 434, trunk time: 334, trunk converged 98, trunk height 65
//! ```
//! * time - The current cluster time.  Each packet is transmitted to the cluster at a different time value.
//! * tip converged - Percentage of nodes voting on the tip.
//! * trunk id - ID of the newest most common fork for the largest converged set of nodes.
//! * trunk time - Time when the trunk fork was created.
//! * trunk converged - Number of voters that have converged on this fork.
//! * trunk height - Ledger height of the trunk.
//!
//!
//! ### Simulating Greedy Choice
//!
//! Parasitic nodes reverse the weighted function and pick the fork that has the least amount of economic finality, but without fully committing to a dead fork.
//!
//! ```
//! // Each run starts with 100 partitions, and it takes about 260 forks for a dominant trunk to emerge
//! // fully parasitic, 5 vote delay, 17% efficient
//! test_with_partitions(100, 0.0, 5, 1.0)
//! time: 1000, tip converged: 100, trunk id: 1095, trunk time: 995, trunk converged 100, trunk height 125
//! // 50% parasitic, 5 vote delay, 30% efficient
//! test_with_partitions(100, 0.0, 5, 0.5)
//! time: 1000, tip converged: 51, trunk id: 1085, trunk time: 985, trunk converged 100, trunk
//! height 223
//! // 25%  parasitic, 5 vote delay, 49% efficient
//! test_with_partitions(100, 0.0, 5, 0.25)
//! time: 1000, tip converged: 79, trunk id: 1096, trunk time: 996, trunk converged 100, trunk
//! height 367
//! // 0%  parasitic, 5 vote delay, 62% efficient
//! test_with_partitions(100, 0.0, 5, 0.0)
//! time: 1000, tip converged: 100, trunk id: 1099, trunk time: 999, trunk converged 100, trunk height 463
//! // 0%  parasitic, 0 vote delay, 100% efficient
//! test_with_partitions(100, 0.0, 0, 0.0)
//! time: 1000, tip converged: 100, trunk id: 1100, trunk time: 1000, trunk converged 100, trunk height 740
//! ```
//!
//! ### Impact of Receive Errors
//!
//! * with 10% of packet drops, the height of the trunk is about 77% of the max possible
//! ```
//! time: 4007, tip converged: 94, trunk id: 4005, trunk time: 4002, trunk converged 100, trunk height 3121
//! ```
//! * with 90% of packet drops, the height of the trunk is about 8.6% of the max possible
//! ```
//! time: 4007, tip converged: 10, trunk id: 3830, trunk time: 3827, trunk converged 100, trunk height 348
//! ```

extern crate rand;
use rand::{thread_rng, Rng};
use std::collections::HashMap;
use std::collections::VecDeque;

#[derive(Clone, Default, Debug, Hash, Eq, PartialEq)]
pub struct Fork {
    id: usize,
    base: usize,
}

impl Fork {
    fn is_trunk_of(&self, other: &Fork, fork_tree: &HashMap<usize, Fork>) -> bool {
        let mut current = other;
        loop {
            // found it
            if current.id == self.id {
                return true;
            }
            // base is 0, and this id is 0
            if current.base == 0 && self.id == 0 {
                assert!(fork_tree.get(&0).is_none());
                return true;
            }
            // base is 0
            if fork_tree.get(&current.base).is_none() {
                return false;
            }
            current = fork_tree.get(&current.base).unwrap();
        }
    }
}

#[derive(Clone, Default, Debug, Hash, Eq, PartialEq)]
pub struct Vote {
    fork: Fork,
    time: usize,
    lockout: usize,
}

impl Vote {
    pub fn new(fork: Fork, time: usize) -> Vote {
        Self {
            fork,
            time,
            lockout: 2,
        }
    }
    pub fn lock_height(&self) -> usize {
        self.time + self.lockout
    }
    pub fn is_trunk_of(&self, other: &Vote, fork_tree: &HashMap<usize, Fork>) -> bool {
        self.fork.is_trunk_of(&other.fork, fork_tree)
    }
}

#[derive(Debug)]
pub struct Tower {
    votes: VecDeque<Vote>,
    max_size: usize,
    fork_trunk: Fork,
    converge_depth: usize,
    delay_count: usize,
    delayed_votes: VecDeque<Vote>,
    parasite: bool,
}

impl Tower {
    pub fn new(max_size: usize, converge_depth: usize, delay_count: usize) -> Self {
        Self {
            votes: VecDeque::new(),
            max_size,
            fork_trunk: Fork::default(),
            converge_depth,
            delay_count,
            delayed_votes: VecDeque::new(),
            parasite: false,
        }
    }
    pub fn submit_vote(
        &mut self,
        vote: Vote,
        fork_tree: &HashMap<usize, Fork>,
        converge_map: &HashMap<usize, usize>,
        scores: &HashMap<Vote, usize>,
    ) {
        let is_valid = self
            .get_vote(self.converge_depth)
            .map(|v| v.is_trunk_of(&vote, fork_tree))
            .unwrap_or(true);
        if is_valid {
            self.delayed_votes.push_front(vote);
        }
        loop {
            if self.delayed_votes.len() <= self.delay_count {
                break;
            }
            let votes = self.pop_best_votes(fork_tree, scores);
            for vote in votes {
                self.push_vote(vote, fork_tree, converge_map);
            }
        }
        let trunk = self.votes.get(self.converge_depth).cloned();
        trunk.map(|t| {
            self.delayed_votes.retain(|v| v.fork.id > t.fork.id);
        });
    }
    pub fn pop_best_votes(
        &mut self,
        fork_tree: &HashMap<usize, Fork>,
        scores: &HashMap<Vote, usize>,
    ) -> VecDeque<Vote> {
        let mut best: Vec<(usize, usize, usize)> = self
            .delayed_votes
            .iter()
            .enumerate()
            .map(|(i, v)| (*scores.get(&v).unwrap_or(&0), v.time, i))
            .collect();
        // highest score, latest vote first
        best.sort();
        if self.parasite {
            best.reverse();
        }
        // best vote is last
        let mut votes: VecDeque<Vote> = best
            .last()
            .and_then(|v| self.delayed_votes.remove(v.2))
            .into_iter()
            .collect();
        // plus any ancestors
        if votes.is_empty() {
            return votes;
        }
        let mut restart = true;
        // should really be using heap here
        while restart {
            restart = false;
            for i in 0..self.delayed_votes.len() {
                let is_trunk = {
                    let v = &self.delayed_votes[i];
                    v.is_trunk_of(votes.front().unwrap(), fork_tree)
                };
                if is_trunk {
                    votes.push_front(self.delayed_votes.remove(i).unwrap());
                    restart = true;
                    break;
                }
            }
        }
        votes
    }
    pub fn push_vote(
        &mut self,
        vote: Vote,
        fork_tree: &HashMap<usize, Fork>,
        converge_map: &HashMap<usize, usize>,
    ) -> bool {
        self.rollback(vote.time);
        if !self.is_valid(&vote, fork_tree) {
            return false;
        }
        if !self.is_converged(converge_map) {
            return false;
        }
        self.process_vote(vote);
        if self.is_full() {
            self.pop_full();
        }
        true
    }
    /// check if the vote at `height` has over 50% of the cluster committed
    fn is_converged(&self, converge_map: &HashMap<usize, usize>) -> bool {
        self.get_vote(self.converge_depth)
            .map(|v| {
                let v = *converge_map.get(&v.fork.id).unwrap_or(&0);
                // hard-coded to 100 nodes
                assert!(v <= 100);
                v > 50
            })
            .unwrap_or(true)
    }

    pub fn score(&self, vote: &Vote, fork_tree: &HashMap<usize, Fork>) -> usize {
        let st = self.rollback_count(vote.time);
        if st < self.votes.len() && !self.votes[st].is_trunk_of(vote, fork_tree) {
            return 0;
        }
        let mut rv = 0;
        for i in st..self.votes.len() {
            let lockout = self.votes[i].lockout;
            rv += lockout;
            if i == 0 || self.votes[i - 1].lockout * 2 == lockout {
                // double the lockout from this vote
                rv += lockout;
            }
        }
        rv
    }

    fn rollback_count(&self, time: usize) -> usize {
        let mut last: usize = 0;
        for (i, v) in self.votes.iter().enumerate() {
            if v.lock_height() < time {
                last = i + 1;
            }
        }
        last
    }
    /// if a vote is expired, pop it and all the votes leading up to it
    fn rollback(&mut self, time: usize) {
        let last = self.rollback_count(time);
        for _ in 0..last {
            self.votes.pop_front();
        }
    }
    /// only add votes that are descendent from the last vote in the stack
    fn is_valid(&self, vote: &Vote, fork_tree: &HashMap<usize, Fork>) -> bool {
        self.last_fork().is_trunk_of(&vote.fork, fork_tree)
    }

    fn process_vote(&mut self, vote: Vote) {
        let vote_time = vote.time;
        assert!(!self.is_full());
        assert_eq!(vote.lockout, 2);
        // push the new vote to the front
        self.votes.push_front(vote);
        // double the lockouts if the threshold to double is met
        for i in 1..self.votes.len() {
            assert!(self.votes[i].time <= vote_time);
            if self.votes[i].lockout == self.votes[i - 1].lockout {
                self.votes[i].lockout *= 2;
            }
        }
    }
    fn pop_full(&mut self) {
        assert!(self.is_full());
        self.fork_trunk = self.votes.pop_back().unwrap().fork;
    }
    fn is_full(&self) -> bool {
        assert!(self.votes.len() <= self.max_size);
        self.votes.len() == self.max_size
    }
    fn last_vote(&self) -> Option<&Vote> {
        self.votes.front()
    }
    fn get_vote(&self, ix: usize) -> Option<&Vote> {
        self.votes.get(ix)
    }
    pub fn first_vote(&self) -> Option<&Vote> {
        self.votes.back()
    }
    pub fn last_fork(&self) -> Fork {
        self.last_vote()
            .map(|v| v.fork.clone())
            .unwrap_or_else(|| self.fork_trunk.clone())
    }
}

#[test]
fn test_is_trunk_of_1() {
    let tree = HashMap::new();
    let b1 = Fork { id: 1, base: 0 };
    let b2 = Fork { id: 2, base: 0 };
    assert!(!b1.is_trunk_of(&b2, &tree));
}
#[test]
fn test_is_trunk_of_2() {
    let tree = HashMap::new();
    let b1 = Fork { id: 1, base: 0 };
    let b2 = Fork { id: 0, base: 0 };
    assert!(!b1.is_trunk_of(&b2, &tree));
}
#[test]
fn test_is_trunk_of_3() {
    let tree = HashMap::new();
    let b1 = Fork { id: 1, base: 0 };
    let b2 = Fork { id: 1, base: 0 };
    assert!(b1.is_trunk_of(&b2, &tree));
}
#[test]
fn test_is_trunk_of_4() {
    let mut tree = HashMap::new();
    let b1 = Fork { id: 1, base: 0 };
    let b2 = Fork { id: 2, base: 1 };
    tree.insert(b1.id, b1.clone());
    assert!(b1.is_trunk_of(&b2, &tree));
    assert!(!b2.is_trunk_of(&b1, &tree));
}
#[test]
fn test_push_vote() {
    let tree = HashMap::new();
    let bmap = HashMap::new();
    let b0 = Fork { id: 0, base: 0 };
    let mut tower = Tower::new(32, 7, 0);
    let vote = Vote::new(b0.clone(), 0);
    assert!(tower.push_vote(vote, &tree, &bmap));
    assert_eq!(tower.votes.len(), 1);

    let vote = Vote::new(b0.clone(), 1);
    assert!(tower.push_vote(vote, &tree, &bmap));
    assert_eq!(tower.votes.len(), 2);

    let vote = Vote::new(b0.clone(), 2);
    assert!(tower.push_vote(vote, &tree, &bmap));
    assert_eq!(tower.votes.len(), 3);

    let vote = Vote::new(b0.clone(), 3);
    assert!(tower.push_vote(vote, &tree, &bmap));
    assert_eq!(tower.votes.len(), 4);

    assert_eq!(tower.votes[0].lockout, 2);
    assert_eq!(tower.votes[1].lockout, 4);
    assert_eq!(tower.votes[2].lockout, 8);
    assert_eq!(tower.votes[3].lockout, 16);

    assert_eq!(tower.votes[1].lock_height(), 6);
    assert_eq!(tower.votes[2].lock_height(), 9);

    let vote = Vote::new(b0.clone(), 7);
    assert!(tower.push_vote(vote, &tree, &bmap));

    assert_eq!(tower.votes[0].lockout, 2);

    let b1 = Fork { id: 1, base: 1 };
    let vote = Vote::new(b1.clone(), 8);
    assert!(!tower.push_vote(vote, &tree, &bmap));

    let vote = Vote::new(b0.clone(), 8);
    assert!(tower.push_vote(vote, &tree, &bmap));

    assert_eq!(tower.votes.len(), 4);
    assert_eq!(tower.votes[0].lockout, 2);
    assert_eq!(tower.votes[1].lockout, 4);
    assert_eq!(tower.votes[2].lockout, 8);
    assert_eq!(tower.votes[3].lockout, 16);

    let vote = Vote::new(b0.clone(), 10);
    assert!(tower.push_vote(vote, &tree, &bmap));
    assert_eq!(tower.votes.len(), 2);
    assert_eq!(tower.votes[0].lockout, 2);
    assert_eq!(tower.votes[1].lockout, 16);
}

fn create_towers(sz: usize, height: usize, delay_count: usize) -> Vec<Tower> {
    (0..sz)
        .into_iter()
        .map(|_| Tower::new(32, height, delay_count))
        .collect()
}

/// The "height" of this fork. How many forks until it connects to fork 0
fn calc_fork_depth(fork_tree: &HashMap<usize, Fork>, id: usize) -> usize {
    let mut height = 0;
    let mut start = fork_tree.get(&id);
    loop {
        if start.is_none() {
            break;
        }
        height += 1;
        start = fork_tree.get(&start.unwrap().base);
    }
    height
}
/// map of `fork id` to `tower count`
/// This map contains the number of nodes that have the fork as an ancestor.
/// The fork with the highest count that is the newest is the cluster "trunk".
fn calc_fork_map(towers: &Vec<Tower>, fork_tree: &HashMap<usize, Fork>) -> HashMap<usize, usize> {
    let mut lca_map: HashMap<usize, usize> = HashMap::new();
    for tower in towers {
        let mut start = tower.last_fork();
        loop {
            *lca_map.entry(start.id).or_insert(0) += 1;
            if !fork_tree.contains_key(&start.base) {
                break;
            }
            start = fork_tree.get(&start.base).unwrap().clone();
        }
    }
    lca_map
}
/// find the fork with the highest count of nodes that have it as an ancestor
/// as well as with the highest possible fork id, which indicates it is the newest
fn calc_newest_trunk(bmap: &HashMap<usize, usize>) -> (usize, usize) {
    let mut data: Vec<_> = bmap.iter().collect();
    data.sort_by_key(|x| (x.1, x.0));
    data.last().map(|v| (*v.0, *v.1)).unwrap()
}
/// how common is the latest fork of all the nodes
fn calc_tip_converged(towers: &Vec<Tower>, bmap: &HashMap<usize, usize>) -> usize {
    let sum: usize = towers
        .iter()
        .map(|n| *bmap.get(&n.last_fork().id).unwrap_or(&0))
        .sum();
    sum / towers.len()
}
#[test]
fn test_no_partitions() {
    let mut tree = HashMap::new();
    let len = 100;
    let mut towers = create_towers(len, 32, 0);
    for rounds in 0..1 {
        for i in 0..towers.len() {
            let time = rounds * len + i;
            let base = towers[i].last_fork().clone();
            let fork = Fork {
                id: time + 1,
                base: base.id,
            };
            tree.insert(fork.id, fork.clone());
            let vote = Vote::new(fork, time);
            let bmap = calc_fork_map(&towers, &tree);
            for tower in towers.iter_mut() {
                assert!(tower.push_vote(vote.clone(), &tree, &bmap));
            }
            println!("{} {}", time, calc_tip_converged(&towers, &bmap));
        }
    }
    let bmap = calc_fork_map(&towers, &tree);
    assert_eq!(calc_tip_converged(&towers, &bmap), len);
}
/// * num_partitions - 1 to 100 partitions
/// * fail_rate - 0 to 1.0 rate of packet receive failure
/// * delay_count - number of forks to observe before voting
/// * parasite_rate - number of parasite nodes that vote oposite the greedy choice
fn test_with_partitions(
    num_partitions: usize,
    fail_rate: f64,
    delay_count: usize,
    parasite_rate: f64,
    break_early: bool,
) {
    let mut fork_tree = HashMap::new();
    let len = 100;
    let warmup = 8;
    let mut towers = create_towers(len, warmup, delay_count);
    for time in 0..warmup {
        let bmap = calc_fork_map(&towers, &fork_tree);
        for tower in towers.iter_mut() {
            let mut fork = tower.last_fork().clone();
            if fork.id == 0 {
                fork.id = thread_rng().gen_range(1, 1 + num_partitions);
                fork_tree.insert(fork.id, fork.clone());
            }
            let vote = Vote::new(fork, time);
            assert!(tower.is_valid(&vote, &fork_tree));
            assert!(tower.push_vote(vote, &fork_tree, &bmap));
        }
    }
    for tower in towers.iter_mut() {
        assert_eq!(tower.votes.len(), warmup);
        assert_eq!(tower.first_vote().unwrap().lockout, 1 << warmup);
        assert!(tower.first_vote().unwrap().lock_height() >= 1 << warmup);
        tower.parasite = parasite_rate > thread_rng().gen_range(0.0, 1.0);
    }
    let converge_map = calc_fork_map(&towers, &fork_tree);
    assert_ne!(calc_tip_converged(&towers, &converge_map), len);
    for rounds in 0..10 {
        for i in 0..len {
            let time = warmup + rounds * len + i;
            let base = towers[i].last_fork();
            let fork = Fork {
                id: time + num_partitions,
                base: base.id,
            };
            fork_tree.insert(fork.id, fork.clone());
            let converge_map = calc_fork_map(&towers, &fork_tree);
            let vote = Vote::new(fork, time);
            let mut scores: HashMap<Vote, usize> = HashMap::new();
            towers.iter().for_each(|n| {
                n.delayed_votes.iter().for_each(|v| {
                    *scores.entry(v.clone()).or_insert(0) += n.score(&v, &fork_tree);
                })
            });
            for tower in towers.iter_mut() {
                if thread_rng().gen_range(0f64, 1.0f64) < fail_rate {
                    continue;
                }
                tower.submit_vote(vote.clone(), &fork_tree, &converge_map, &scores);
            }
            let converge_map = calc_fork_map(&towers, &fork_tree);
            let trunk = calc_newest_trunk(&converge_map);
            let trunk_time = if trunk.0 > num_partitions {
                trunk.0 - num_partitions
            } else {
                trunk.0
            };
            println!(
                    "time: {}, tip converged: {}, trunk id: {}, trunk time: {}, trunk converged {}, trunk height {}",
                    time,
                    calc_tip_converged(&towers, &converge_map),
                    trunk.0,
                    trunk_time,
                    trunk.1,
                    calc_fork_depth(&fork_tree, trunk.0)
                );
            if break_early && calc_tip_converged(&towers, &converge_map) == len {
                break;
            }
        }
        if break_early {
            let converge_map = calc_fork_map(&towers, &fork_tree);
            if calc_tip_converged(&towers, &converge_map) == len {
                break;
            }
        }
    }
    let converge_map = calc_fork_map(&towers, &fork_tree);
    let trunk = calc_newest_trunk(&converge_map);
    assert_eq!(trunk.1, len);
}

#[test]
#[ignore]
fn test_3_partitions() {
    test_with_partitions(3, 0.0, 0, 0.0, true)
}
#[test]
#[ignore]
fn test_3_partitions_large_packet_drop() {
    test_with_partitions(3, 0.9, 0, 0.0, false)
}
#[test]
#[ignore]
fn test_all_partitions() {
    test_with_partitions(100, 0.0, 5, 0.25, false)
}
