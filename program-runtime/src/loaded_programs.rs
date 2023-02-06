use std::fmt::{Debug, Formatter};
use {
    crate::invoke_context::InvokeContext,
    solana_rbpf::{
        elf::Executable,
        error::EbpfError,
        verifier::RequisiteVerifier,
        vm::{BuiltInProgram, VerifiedExecutable},
    },
    solana_sdk::{
        bpf_loader, bpf_loader_deprecated, bpf_loader_upgradeable, clock::Slot, pubkey::Pubkey,
    },
    std::{
        collections::HashMap,
        sync::{atomic::AtomicU64, Arc},
    },
};

/// Relationship between two fork IDs
#[derive(Copy, Clone, PartialEq)]
pub enum BlockRelation {
    /// The slot is on the same fork and is an ancestor of the other slot
    Ancestor,
    /// The two slots are same and are on the same fork
    Equal,
    /// The slot is on the same fork and is a descendant of the other slot
    Descendant,
    /// The slots are on two different forks and may have had a common ancestor at some point
    Unrelated,
    /// Either or both of the slots are either older than the latest root, or are in future
    Unknown,
}

/// Maps relationship between two slots.
pub trait ForkGraph {
    /// Returns the BlockRelation of A to B
    fn relationship(&self, a: Slot, b: Slot) -> BlockRelation;
}

/// Provides information about current working slot, and its ancestors
pub trait WorkingSlot {
    /// Returns the current slot value
    fn current_slot(&self) -> Slot;

    /// Returns true if the `other` slot is an ancestor of self, false otherwise
    fn is_ancestor(&self, other: Slot) -> bool;
}

pub enum LoadedProgramType {
    /// Tombstone for undeployed, closed or unloadable programs
    Invalid,
    LegacyV0(VerifiedExecutable<RequisiteVerifier, InvokeContext<'static>>),
    LegacyV1(VerifiedExecutable<RequisiteVerifier, InvokeContext<'static>>),
    // Typed(TypedProgram<InvokeContext<'static>>),
    BuiltIn(BuiltInProgram<InvokeContext<'static>>),
}

pub struct LoadedProgram {
    /// The program of this entry
    pub program: LoadedProgramType,
    /// Slot in which the program was (re)deployed
    pub deployment_slot: Slot,
    /// Slot in which this entry will become active (can be in the future)
    pub effective_slot: Slot,
    /// How often this entry was used
    pub usage_counter: AtomicU64,
}

impl LoadedProgram {
    /// Creates a new user program
    pub fn new(
        loader_key: &Pubkey,
        loader: Arc<BuiltInProgram<InvokeContext<'static>>>,
        deployment_slot: Slot,
        elf_bytes: &[u8],
    ) -> Result<Self, EbpfError> {
        let program = if bpf_loader_deprecated::check_id(loader_key) {
            let executable = Executable::load(elf_bytes, loader.clone())?;
            LoadedProgramType::LegacyV0(VerifiedExecutable::from_executable(executable)?)
        } else if bpf_loader::check_id(loader_key) || bpf_loader_upgradeable::check_id(loader_key) {
            let executable = Executable::load(elf_bytes, loader.clone())?;
            LoadedProgramType::LegacyV1(VerifiedExecutable::from_executable(executable)?)
        } else {
            panic!();
        };
        Ok(Self {
            deployment_slot,
            effective_slot: deployment_slot + 1,
            usage_counter: AtomicU64::new(0),
            program,
        })
    }

    /// Creates a new built-in program
    pub fn new_built_in(
        deployment_slot: Slot,
        program: BuiltInProgram<InvokeContext<'static>>,
    ) -> Self {
        Self {
            deployment_slot,
            effective_slot: deployment_slot + 1,
            usage_counter: AtomicU64::new(0),
            program: LoadedProgramType::BuiltIn(program),
        }
    }
}

#[derive(Default)]
pub struct LoadedPrograms {
    /// A two level index:
    ///
    /// Pubkey is the address of a program, multiple versions can coexists simultaneously under the same address (in different slots).
    entries: HashMap<Pubkey, Vec<Arc<LoadedProgram>>>,
}

impl Debug for LoadedPrograms {
    fn fmt(&self, _f: &mut Formatter<'_>) -> std::fmt::Result {
        todo!()
    }
}

impl LoadedPrograms {
    /// Inserts a single entry
    pub fn insert_entry(&mut self, key: Pubkey, entry: LoadedProgram) -> bool {
        let second_level = self.entries.entry(key).or_insert_with(Vec::new);
        let index = second_level
            .iter()
            .position(|at| at.effective_slot >= entry.effective_slot);
        if let Some(index) = index {
            if second_level[index].deployment_slot == entry.deployment_slot
                && second_level[index].effective_slot == entry.effective_slot
            {
                return false;
            }
        }
        second_level.insert(index.unwrap_or(second_level.len()), Arc::new(entry));
        return true;
    }

    /// Before rerooting the blockstore this removes all programs of orphan forks
    pub fn prune<F: ForkGraph>(&mut self, fork_graph: &F, new_root: Slot) {
        self.entries.retain(|_key, second_level| {
            let mut first_ancestor = true;
            *second_level = second_level
                .iter()
                .rev()
                .filter(|entry| {
                    let relation = fork_graph.relationship(entry.deployment_slot, new_root);
                    if entry.deployment_slot >= new_root {
                        matches!(relation, BlockRelation::Equal | BlockRelation::Descendant)
                    } else if first_ancestor {
                        first_ancestor = false;
                        matches!(relation, BlockRelation::Ancestor)
                    } else {
                        false
                    }
                })
                .cloned()
                .collect();
            second_level.reverse();
            !second_level.is_empty()
        });
    }

    /// Extracts a subset of the programs relevant to a transaction batch
    /// and returns which program accounts the accounts DB needs to load.
    pub fn extract<S: WorkingSlot>(
        &self,
        working_slot: &S,
        keys: impl Iterator<Item = Pubkey>,
    ) -> (HashMap<Pubkey, Arc<LoadedProgram>>, Vec<Pubkey>) {
        let mut missing = Vec::new();
        let found = keys
            .filter_map(|key| {
                if let Some(second_level) = self.entries.get(&key) {
                    for entry in second_level.iter().rev() {
                        if working_slot.is_ancestor(entry.deployment_slot) {
                            return Some((key, entry.clone()));
                        }
                    }
                }
                missing.push(key);
                None
            })
            .collect();
        (found, missing)
    }

    /// Evicts programs which were used infrequently
    pub fn sort_and_evict(&mut self) {
        // TODO: Sort programs by their usage_counter
        // TODO: Truncate the end of the list
    }

    /// Removes the entries at the given keys, if they exist
    pub fn remove_entries(&mut self, _key: impl Iterator<Item = Pubkey>) {
        // TODO: Remove at primary index level
    }
}

#[cfg(test)]
mod tests {
    use crate::loaded_programs::{
        BlockRelation, ForkGraph, LoadedProgram, LoadedProgramType, LoadedPrograms, WorkingSlot,
    };
    use solana_sdk::clock::Slot;
    use solana_sdk::pubkey::Pubkey;
    use std::ops::ControlFlow;
    use std::sync::atomic::AtomicU64;
    use std::sync::Arc;

    struct TestForkGraph {
        relation: BlockRelation,
    }
    impl ForkGraph for TestForkGraph {
        fn relationship(&self, _a: Slot, _b: Slot) -> BlockRelation {
            self.relation
        }
    }

    #[test]
    fn test_prune_empty() {
        let mut cache = LoadedPrograms::default();
        let fork_graph = TestForkGraph {
            relation: BlockRelation::Unrelated,
        };

        cache.prune(&fork_graph, 0);
        assert!(cache.entries.is_empty());

        cache.prune(&fork_graph, 10);
        assert!(cache.entries.is_empty());

        let fork_graph = TestForkGraph {
            relation: BlockRelation::Ancestor,
        };

        cache.prune(&fork_graph, 0);
        assert!(cache.entries.is_empty());

        cache.prune(&fork_graph, 10);
        assert!(cache.entries.is_empty());

        let fork_graph = TestForkGraph {
            relation: BlockRelation::Descendant,
        };

        cache.prune(&fork_graph, 0);
        assert!(cache.entries.is_empty());

        cache.prune(&fork_graph, 10);
        assert!(cache.entries.is_empty());

        let fork_graph = TestForkGraph {
            relation: BlockRelation::Unknown,
        };

        cache.prune(&fork_graph, 0);
        assert!(cache.entries.is_empty());

        cache.prune(&fork_graph, 10);
        assert!(cache.entries.is_empty());
    }

    #[derive(Default)]
    struct TestForkGraphSpecific {
        forks: Vec<Vec<Slot>>,
    }

    impl TestForkGraphSpecific {
        fn insert_fork(&mut self, fork: &[Slot]) {
            let mut fork = fork.to_vec();
            fork.sort();
            self.forks.push(fork)
        }
    }

    impl ForkGraph for TestForkGraphSpecific {
        fn relationship(&self, a: Slot, b: Slot) -> BlockRelation {
            match self.forks.iter().try_for_each(|fork| {
                let relation = fork
                    .iter()
                    .position(|x| *x == a)
                    .and_then(|a_pos| {
                        fork.iter().position(|x| *x == b).and_then(|b_pos| {
                            (a_pos == b_pos)
                                .then(|| BlockRelation::Equal)
                                .or_else(|| (a_pos < b_pos).then(|| BlockRelation::Ancestor))
                                .or(Some(BlockRelation::Descendant))
                        })
                    })
                    .unwrap_or(BlockRelation::Unrelated);

                if relation != BlockRelation::Unrelated {
                    return ControlFlow::Break(relation);
                }

                ControlFlow::Continue(())
            }) {
                ControlFlow::Break(relation) => relation,
                _ => BlockRelation::Unrelated,
            }
        }
    }

    struct TestWorkingSlot {
        slot: Slot,
        fork: Vec<Slot>,
        slot_pos: usize,
    }

    impl TestWorkingSlot {
        fn new(slot: Slot, fork: &[Slot]) -> Self {
            let mut fork = fork.to_vec();
            fork.sort();
            let slot_pos = fork
                .iter()
                .position(|current| *current == slot)
                .expect("The fork didn't have the slot in it");
            TestWorkingSlot {
                slot,
                fork,
                slot_pos,
            }
        }
    }

    impl WorkingSlot for TestWorkingSlot {
        fn current_slot(&self) -> Slot {
            self.slot
        }

        fn is_ancestor(&self, other: Slot) -> bool {
            self.fork
                .iter()
                .position(|current| *current == other)
                .and_then(|other_pos| Some(other_pos < self.slot_pos))
                .unwrap_or(false)
        }
    }

    fn new_test_loaded_program(deployment_slot: Slot) -> Arc<LoadedProgram> {
        Arc::new(LoadedProgram {
            program: LoadedProgramType::Invalid,
            deployment_slot,
            effective_slot: 0,
            usage_counter: AtomicU64::default(),
        })
    }

    #[test]
    fn test_fork_extract_and_prune() {
        let mut cache = LoadedPrograms::default();

        // Fork graph created for the test
        //                   0
        //                 /   \
        //                10    5
        //                |     |
        //                20    11
        //                |     | \
        //                22   15  25
        //                      |   |
        //                     16  27

        let mut fork_graph = TestForkGraphSpecific::default();
        fork_graph.insert_fork(&[0, 10, 20, 22]);
        fork_graph.insert_fork(&[0, 5, 11, 15, 16]);
        fork_graph.insert_fork(&[0, 5, 11, 25, 27]);

        let program1 = Pubkey::new_unique();
        cache.entries.insert(
            program1,
            vec![
                new_test_loaded_program(0),
                new_test_loaded_program(10),
                new_test_loaded_program(20),
            ],
        );

        let program2 = Pubkey::new_unique();
        cache.entries.insert(
            program2,
            vec![new_test_loaded_program(5), new_test_loaded_program(11)],
        );

        let program3 = Pubkey::new_unique();
        cache
            .entries
            .insert(program3, vec![new_test_loaded_program(25)]);

        let program4 = Pubkey::new_unique();
        cache.entries.insert(
            program4,
            vec![
                new_test_loaded_program(0),
                new_test_loaded_program(5),
                new_test_loaded_program(15),
            ],
        );

        // Current fork graph
        //                   0
        //                 /   \
        //                10    5
        //                |     |
        //                20    11
        //                |     | \
        //                22   15  25
        //                      |   |
        //                     16  27

        // Testing fork 0 - 10 - 12 - 22 with current slot at 22
        let working_slot = TestWorkingSlot::new(22, &[0, 10, 20, 22]);
        let (found, missing) = cache.extract(
            &working_slot,
            vec![program1, program2, program3, program4].into_iter(),
        );

        assert!(found.contains_key(&program1));
        assert_eq!(found[&program1].deployment_slot, 20);

        assert!(found.contains_key(&program4));
        assert_eq!(found[&program4].deployment_slot, 0);

        assert!(missing.contains(&program2));
        assert!(missing.contains(&program3));

        // Testing fork 0 - 5 - 11 - 15 - 16 with current slot at 16
        let working_slot = TestWorkingSlot::new(16, &[0, 5, 11, 15, 16]);
        let (found, missing) = cache.extract(
            &working_slot,
            vec![program1, program2, program3, program4].into_iter(),
        );

        assert!(found.contains_key(&program1));
        assert_eq!(found[&program1].deployment_slot, 0);

        assert!(found.contains_key(&program2));
        assert_eq!(found[&program2].deployment_slot, 11);

        assert!(found.contains_key(&program4));
        assert_eq!(found[&program4].deployment_slot, 15);

        assert!(missing.contains(&program3));

        // Testing fork 0 - 5 - 11 - 15 - 16 with current slot at 11
        let working_slot = TestWorkingSlot::new(11, &[0, 5, 11, 15, 16]);
        let (found, missing) = cache.extract(
            &working_slot,
            vec![program1, program2, program3, program4].into_iter(),
        );

        assert!(found.contains_key(&program1));
        assert_eq!(found[&program1].deployment_slot, 0);

        assert!(found.contains_key(&program2));
        assert_eq!(found[&program2].deployment_slot, 5);

        assert!(found.contains_key(&program4));
        assert_eq!(found[&program4].deployment_slot, 5);

        assert!(missing.contains(&program3));

        cache.prune(&fork_graph, 5);

        // Fork graph after pruning
        //                   0
        //                   |
        //                   5
        //                   |
        //                   11
        //                   | \
        //                  15  25
        //                   |   |
        //                  16  27

        // Testing fork 0 - 10 - 12 - 22 (which was pruned) with current slot at 22
        let working_slot = TestWorkingSlot::new(22, &[0, 10, 20, 22]);
        let (found, missing) = cache.extract(
            &working_slot,
            vec![program1, program2, program3, program4].into_iter(),
        );

        assert!(found.contains_key(&program1));
        // Since the fork was pruned, we should not find the entry deployed at slot 20.
        assert_eq!(found[&program1].deployment_slot, 0);

        assert!(found.contains_key(&program4));
        assert_eq!(found[&program4].deployment_slot, 0);

        assert!(missing.contains(&program2));
        assert!(missing.contains(&program3));

        // Testing fork 0 - 5 - 11 - 25 - 27 with current slot at 27
        let working_slot = TestWorkingSlot::new(27, &[0, 5, 11, 25, 27]);
        let (found, _missing) = cache.extract(
            &working_slot,
            vec![program1, program2, program3, program4].into_iter(),
        );

        assert!(found.contains_key(&program1));
        assert_eq!(found[&program1].deployment_slot, 0);

        assert!(found.contains_key(&program2));
        assert_eq!(found[&program2].deployment_slot, 11);

        assert!(found.contains_key(&program3));
        assert_eq!(found[&program3].deployment_slot, 25);

        assert!(found.contains_key(&program4));
        assert_eq!(found[&program4].deployment_slot, 5);

        cache.prune(&fork_graph, 15);

        // Fork graph after pruning
        //                  0
        //                  |
        //                  5
        //                  |
        //                  11
        //                  |
        //                  15
        //                  |
        //                  16

        // Testing fork 0 - 5 - 11 - 25 - 27 (with root at 15, slot 25, 27 are pruned) with current slot at 27
        let working_slot = TestWorkingSlot::new(27, &[0, 5, 11, 25, 27]);
        let (found, missing) = cache.extract(
            &working_slot,
            vec![program1, program2, program3, program4].into_iter(),
        );

        assert!(found.contains_key(&program1));
        assert_eq!(found[&program1].deployment_slot, 0);

        assert!(found.contains_key(&program2));
        assert_eq!(found[&program2].deployment_slot, 11);

        assert!(found.contains_key(&program4));
        assert_eq!(found[&program4].deployment_slot, 5);

        // program3 was deployed on slot 25, which has been pruned
        assert!(missing.contains(&program3));
    }
}
