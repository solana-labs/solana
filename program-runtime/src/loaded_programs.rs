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
