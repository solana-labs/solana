#[macro_use]
extern crate solana_bpf_loader_program;
#[macro_use]
extern crate solana_budget_program;
#[macro_use]
extern crate solana_exchange_program;
#[macro_use]
extern crate solana_vest_program;

use solana_runtime::bank::{Bank, EnteredEpochCallback};
use solana_sdk::{
    clock::{Epoch, GENESIS_EPOCH},
    entrypoint_native::{ErasedProcessInstructionWithContext, ProcessInstructionWithContext},
    genesis_config::ClusterType,
    pubkey::Pubkey,
};

enum Program {
    Native((String, Pubkey)),
    BuiltinLoader((String, Pubkey, ProcessInstructionWithContext)),
}

impl std::fmt::Debug for Program {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        #[derive(Debug)]
        enum Program {
            Native((String, Pubkey)),
            BuiltinLoader((String, Pubkey, String)),
        }
        let program = match self {
            crate::Program::Native((string, pubkey)) => Program::Native((string.clone(), *pubkey)),
            crate::Program::BuiltinLoader((string, pubkey, instruction)) => {
                let erased: ErasedProcessInstructionWithContext = *instruction;
                Program::BuiltinLoader((string.clone(), *pubkey, format!("{:p}", erased)))
            }
        };
        write!(f, "{:?}", program)
    }
}

// given cluster type, return the entire set of enabled programs
fn get_programs(cluster_type: ClusterType) -> Vec<(Program, Epoch)> {
    match cluster_type {
        ClusterType::Development => vec![
            // Programs used for testing
            Program::BuiltinLoader(solana_bpf_loader_program!()),
            Program::BuiltinLoader(solana_bpf_loader_deprecated_program!()),
            Program::Native(solana_vest_program!()),
            Program::Native(solana_budget_program!()),
            Program::Native(solana_exchange_program!()),
        ]
        .into_iter()
        .map(|program| (program, GENESIS_EPOCH))
        .collect::<Vec<_>>(),

        ClusterType::Devnet => vec![
            (
                Program::BuiltinLoader(solana_bpf_loader_deprecated_program!()),
                GENESIS_EPOCH,
            ),
            (Program::Native(solana_vest_program!()), GENESIS_EPOCH),
            (Program::Native(solana_budget_program!()), GENESIS_EPOCH),
            (Program::Native(solana_exchange_program!()), GENESIS_EPOCH),
            (Program::BuiltinLoader(solana_bpf_loader_program!()), 400),
        ],

        ClusterType::Testnet => vec![
            (
                Program::BuiltinLoader(solana_bpf_loader_deprecated_program!()),
                GENESIS_EPOCH,
            ),
            (Program::BuiltinLoader(solana_bpf_loader_program!()), 89),
        ],

        ClusterType::MainnetBeta => vec![
            (
                Program::BuiltinLoader(solana_bpf_loader_deprecated_program!()),
                34,
            ),
            (
                Program::BuiltinLoader(solana_bpf_loader_program!()),
                // The epoch of std::u64::MAX is a placeholder and is expected
                // to be reduced in a future cluster update.
                Epoch::MAX,
            ),
        ],
    }
}

pub fn get_native_programs_for_genesis(cluster_type: ClusterType) -> Vec<(String, Pubkey)> {
    let mut native_programs = vec![];
    for (program, start_epoch) in get_programs(cluster_type) {
        if let Program::Native((string, key)) = program {
            if start_epoch == GENESIS_EPOCH {
                native_programs.push((string, key));
            }
        }
    }
    native_programs
}

pub fn get_entered_epoch_callback(cluster_type: ClusterType) -> EnteredEpochCallback {
    Box::new(move |bank: &mut Bank, initial: bool| {
        // Be careful to add arbitrary logic here; this should be idempotent and can be called
        // at arbitrary point in an epoch not only epoch boundaries.
        // This is because this closure need to be executed immediately after snapshot restoration,
        // in addition to usual epoch boundaries
        // In other words, this callback initializes some skip(serde) fields, regardless
        // frozen or not

        for (program, start_epoch) in get_programs(cluster_type) {
            let should_populate =
                initial && bank.epoch() >= start_epoch || !initial && bank.epoch() == start_epoch;
            if should_populate {
                match program {
                    Program::Native((name, program_id)) => {
                        bank.add_native_program(&name, &program_id);
                    }
                    Program::BuiltinLoader((
                        name,
                        program_id,
                        process_instruction_with_context,
                    )) => {
                        bank.add_builtin_loader(
                            &name,
                            program_id,
                            process_instruction_with_context,
                        );
                    }
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn do_test_uniqueness(programs: Vec<(Program, Epoch)>) {
        let mut unique_ids = HashSet::new();
        let mut unique_names = HashSet::new();
        let mut prev_start_epoch = GENESIS_EPOCH;
        for (program, next_start_epoch) in programs {
            assert!(next_start_epoch >= prev_start_epoch);
            match program {
                Program::Native((name, id)) => {
                    assert!(unique_ids.insert(id));
                    assert!(unique_names.insert(name));
                }
                Program::BuiltinLoader((name, id, _)) => {
                    assert!(unique_ids.insert(id));
                    assert!(unique_names.insert(name));
                }
            }
            prev_start_epoch = next_start_epoch;
        }
    }

    #[test]
    fn test_uniqueness() {
        do_test_uniqueness(get_programs(ClusterType::Development));
        do_test_uniqueness(get_programs(ClusterType::Devnet));
        do_test_uniqueness(get_programs(ClusterType::Testnet));
        do_test_uniqueness(get_programs(ClusterType::MainnetBeta));
    }

    #[test]
    fn test_development_programs() {
        assert_eq!(get_programs(ClusterType::Development).len(), 5);
    }

    #[test]
    fn test_native_development_programs() {
        assert_eq!(
            get_native_programs_for_genesis(ClusterType::Development).len(),
            3
        );
    }
    #[test]
    fn test_softlaunch_programs() {
        assert!(!get_programs(ClusterType::MainnetBeta).is_empty());
    }

    #[test]
    fn test_debug() {
        assert!(!format!("{:?}", get_programs(ClusterType::Development)).is_empty());
    }
}
