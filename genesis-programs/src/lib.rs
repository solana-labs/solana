use solana_sdk::{
    clock::Epoch, genesis_config::OperatingMode, inflation::Inflation,
    move_loader::solana_move_loader_program, pubkey::Pubkey,
};

#[macro_use]
extern crate solana_bpf_loader_program;
#[macro_use]
extern crate solana_budget_program;
#[macro_use]
extern crate solana_exchange_program;
#[macro_use]
extern crate solana_vest_program;

use log::*;
use solana_runtime::bank::{Bank, EnteredEpochCallback};

pub fn get_inflation(operating_mode: OperatingMode, epoch: Epoch) -> Option<Inflation> {
    match operating_mode {
        OperatingMode::Development => {
            if epoch == 0 {
                Some(Inflation::default())
            } else {
                None
            }
        }
        OperatingMode::Preview => {
            if epoch == 0 {
                // No inflation at epoch 0
                Some(Inflation::new_disabled())
            } else if epoch == 44 {
                // testnet enabled inflation at epoch 44:
                // https://github.com/solana-labs/solana/blob/d8e885f4259e6c7db420cce513cb34ebf961073d
                Some(Inflation::default())
            } else if epoch == 68 {
                // Completely disable inflation prior to ship the inflation fix at epoch 68
                Some(Inflation::new_disabled())
            } else {
                None
            }
        }
        OperatingMode::Stable => {
            if epoch == 0 {
                // No inflation at epoch 0
                Some(Inflation::new_disabled())
            } else if epoch == std::u64::MAX {
                // Inflation starts
                // The epoch of std::u64::MAX is a placeholder and is expected to be reduced in
                // a future hard fork.
                Some(Inflation::default())
            } else {
                None
            }
        }
    }
}

pub fn get_programs(operating_mode: OperatingMode, epoch: Epoch) -> Option<Vec<(String, Pubkey)>> {
    match operating_mode {
        OperatingMode::Development => {
            if epoch == 0 {
                Some(vec![
                    // Enable all Stable programs
                    solana_bpf_loader_program!(),
                    solana_vest_program!(),
                    // Programs that are only available in Development mode
                    solana_budget_program!(),
                    solana_exchange_program!(),
                    solana_move_loader_program(),
                ])
            } else {
                None
            }
        }
        OperatingMode::Stable => {
            if epoch == std::u64::MAX - 1 {
                // The epoch of std::u64::MAX - 1 is a placeholder and is expected to be reduced in
                // a future hard fork.
                Some(vec![solana_bpf_loader_program!()])
            } else if epoch == std::u64::MAX {
                // The epoch of std::u64::MAX is a placeholder and is expected to be reduced in a
                // future hard fork.
                Some(vec![solana_vest_program!()])
            } else {
                None
            }
        }
        OperatingMode::Preview => {
            if epoch == 0 {
                Some(vec![solana_bpf_loader_program!()])
            } else if epoch == std::u64::MAX {
                // The epoch of std::u64::MAX is a placeholder and is expected to be reduced in a
                // future hard fork.
                Some(vec![solana_vest_program!()])
            } else {
                None
            }
        }
    }
}

pub fn get_entered_epoch_callback(operating_mode: OperatingMode) -> EnteredEpochCallback {
    Box::new(move |bank: &mut Bank| {
        info!(
            "Entering epoch {} with operating_mode {:?}",
            bank.epoch(),
            operating_mode
        );
        if let Some(inflation) = get_inflation(operating_mode, bank.epoch()) {
            info!("Entering new epoch with inflation {:?}", inflation);
            bank.set_inflation(inflation);
        }
        if let Some(new_programs) = get_programs(operating_mode, bank.epoch()) {
            for (name, program_id) in new_programs.iter() {
                info!("Registering {} at {}", name, program_id);
                bank.add_native_program(name, program_id);
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_id_uniqueness() {
        let mut unique = HashSet::new();
        let ids = get_programs(OperatingMode::Development, 0).unwrap();
        assert!(ids.into_iter().all(move |id| unique.insert(id)));
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
        assert_eq!(get_programs(OperatingMode::Development, 1), None);
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
        assert_eq!(get_programs(OperatingMode::Stable, 1), None);
        assert!(get_programs(OperatingMode::Stable, std::u64::MAX - 1).is_some());
        assert!(get_programs(OperatingMode::Stable, std::u64::MAX).is_some());
    }
}
