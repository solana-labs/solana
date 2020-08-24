#[macro_use]
extern crate solana_bpf_loader_program;
#[macro_use]
extern crate solana_budget_program;
#[macro_use]
extern crate solana_exchange_program;
#[macro_use]
extern crate solana_vest_program;

use log::*;
use solana_runtime::{
    bank::{Bank, EnteredEpochCallback},
    message_processor::{DEFAULT_COMPUTE_BUDGET, DEFAULT_MAX_INVOKE_DEPTH},
};
use solana_sdk::{
    clock::Epoch, entrypoint_native::ProcessInstructionWithContext, genesis_config::OperatingMode,
    inflation::Inflation, pubkey::Pubkey,
};

pub fn get_inflation(operating_mode: OperatingMode, epoch: Epoch) -> Option<Inflation> {
    match operating_mode {
        OperatingMode::Development => match epoch {
            0 => Some(Inflation::default()),
            _ => None,
        },
        OperatingMode::Preview => match epoch {
            // No inflation at epoch 0
            0 => Some(Inflation::new_disabled()),
            // testnet enabled inflation at epoch 44:
            // https://github.com/solana-labs/solana/commit/d8e885f4259e6c7db420cce513cb34ebf961073d
            44 => Some(Inflation::default()),
            // Completely disable inflation prior to ship the inflation fix at epoch 68
            68 => Some(Inflation::new_disabled()),
            // Enable again after the inflation fix has landed:
            // https://github.com/solana-labs/solana/commit/7cc2a6801bed29a816ef509cfc26a6f2522e46ff
            74 => Some(Inflation::default()),
            _ => None,
        },
        OperatingMode::Stable => match epoch {
            // No inflation at epoch 0
            0 => Some(Inflation::new_disabled()),
            // Inflation starts The epoch of Epoch::MAX is a placeholder and is
            // expected to be reduced in a future hard fork.
            Epoch::MAX => Some(Inflation::default()),
            _ => None,
        },
    }
}

enum Program {
    Native((String, Pubkey)),
    BuiltinLoader((String, Pubkey, ProcessInstructionWithContext)),
}

fn get_programs(operating_mode: OperatingMode, epoch: Epoch) -> Option<Vec<Program>> {
    match operating_mode {
        OperatingMode::Development => {
            if epoch == 0 {
                // Programs used for testing
                Some(vec![
                    Program::BuiltinLoader(solana_bpf_loader_program!()),
                    Program::BuiltinLoader(solana_bpf_loader_deprecated_program!()),
                    Program::Native(solana_vest_program!()),
                    Program::Native(solana_budget_program!()),
                    Program::Native(solana_exchange_program!()),
                ])
            } else if epoch == std::u64::MAX {
                // The epoch of std::u64::MAX is a placeholder and is expected
                // to be reduced in a future network update.
                Some(vec![Program::BuiltinLoader(solana_bpf_loader_program!())])
            } else {
                None
            }
        }
        OperatingMode::Stable => {
            if epoch == std::u64::MAX {
                // The epoch of std::u64::MAX is a placeholder and is expected
                // to be reduced in a future network update.
                Some(vec![
                    Program::BuiltinLoader(solana_bpf_loader_program!()),
                    Program::Native(solana_vest_program!()),
                ])
            } else {
                None
            }
        }
        OperatingMode::Preview => {
            if epoch == std::u64::MAX {
                // The epoch of std::u64::MAX is a placeholder and is expected
                // to be reduced in a future network update.
                Some(vec![
                    Program::BuiltinLoader(solana_bpf_loader_program!()),
                    Program::Native(solana_vest_program!()),
                ])
            } else {
                None
            }
        }
    }
}

pub fn get_native_programs(
    operating_mode: OperatingMode,
    epoch: Epoch,
) -> Option<Vec<(String, Pubkey)>> {
    match get_programs(operating_mode, epoch) {
        Some(programs) => {
            let mut native_programs = vec![];
            for program in programs {
                if let Program::Native((string, key)) = program {
                    native_programs.push((string, key));
                }
            }
            Some(native_programs)
        }
        None => None,
    }
}

pub fn get_entered_epoch_callback(operating_mode: OperatingMode) -> EnteredEpochCallback {
    Box::new(move |bank: &mut Bank| {
        if let Some(inflation) = get_inflation(operating_mode, bank.epoch()) {
            info!("Entering new epoch with inflation {:?}", inflation);
            bank.set_inflation(inflation);
        }
        if let Some(programs) = get_programs(operating_mode, bank.epoch()) {
            for program in programs {
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
        if OperatingMode::Stable == operating_mode {
            bank.set_cross_program_support(bank.epoch() >= 63);
        } else {
            bank.set_cross_program_support(true);
        }

        bank.set_max_invoke_depth(DEFAULT_MAX_INVOKE_DEPTH);
        bank.set_compute_budget(DEFAULT_COMPUTE_BUDGET);
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_id_uniqueness() {
        let mut unique = HashSet::new();
        let programs = get_programs(OperatingMode::Development, 0).unwrap();
        for program in programs {
            match program {
                Program::Native((name, id)) => assert!(unique.insert((name, id))),
                Program::BuiltinLoader((name, id, _)) => assert!(unique.insert((name, id))),
            }
        }
    }

    #[test]
    fn test_development_inflation() {
        assert_eq!(
            get_inflation(OperatingMode::Development, 0).unwrap(),
            Inflation::default()
        );
        assert_eq!(get_inflation(OperatingMode::Development, 1), None);
    }

    #[test]
    fn test_development_programs() {
        assert_eq!(
            get_programs(OperatingMode::Development, 0).unwrap().len(),
            5
        );
        assert!(get_programs(OperatingMode::Development, 1).is_none());
    }

    #[test]
    fn test_native_development_programs() {
        assert_eq!(
            get_native_programs(OperatingMode::Development, 0)
                .unwrap()
                .len(),
            3
        );
        assert!(get_native_programs(OperatingMode::Development, 1).is_none());
    }

    #[test]
    fn test_softlaunch_inflation() {
        assert_eq!(
            get_inflation(OperatingMode::Stable, 0).unwrap(),
            Inflation::new_disabled()
        );
        assert_eq!(get_inflation(OperatingMode::Stable, 1), None);
        assert_eq!(
            get_inflation(OperatingMode::Stable, std::u64::MAX).unwrap(),
            Inflation::default()
        );
    }

    #[test]
    fn test_softlaunch_programs() {
        assert!(get_programs(OperatingMode::Stable, 1).is_none());
        assert!(get_programs(OperatingMode::Stable, std::u64::MAX).is_some());
    }
}
