use solana_sdk::{
    clock::Epoch, genesis_config::OperatingMode, inflation::Inflation,
    move_loader::solana_move_loader_program, nonce_program::solana_nonce_program, pubkey::Pubkey,
    system_program::solana_system_program,
};

#[macro_use]
extern crate solana_bpf_loader_program;
#[macro_use]
extern crate solana_budget_program;
#[macro_use]
extern crate solana_config_program;
#[macro_use]
extern crate solana_exchange_program;
#[macro_use]
extern crate solana_stake_program;
#[macro_use]
extern crate solana_storage_program;
#[macro_use]
extern crate solana_vest_program;
#[macro_use]
extern crate solana_vote_program;

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
        OperatingMode::SoftLaunch => {
            if epoch == 0 {
                // No inflation at epoch 0
                Some(Inflation::new_disabled())
            } else if epoch == std::u64::MAX {
                // Inflation starts
                //
                // The epoch of std::u64::MAX - 1 is a placeholder and is expected to be reduced in
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
                    // Enable all SoftLaunch programs
                    solana_system_program(),
                    solana_bpf_loader_program!(),
                    solana_config_program!(),
                    solana_nonce_program(),
                    solana_stake_program!(),
                    solana_storage_program!(),
                    solana_vest_program!(),
                    solana_vote_program!(),
                    // Programs that are only available in Development mode
                    solana_budget_program!(),
                    solana_exchange_program!(),
                    solana_move_loader_program(),
                ])
            } else {
                None
            }
        }
        OperatingMode::SoftLaunch => {
            if epoch == 0 {
                // Nonce, Voting, Staking and System Program only at epoch 0
                Some(vec![
                    solana_nonce_program(),
                    solana_stake_program!(),
                    solana_system_program(),
                    solana_vote_program!(),
                ])
            } else if epoch == std::u64::MAX - 1 {
                // Archivers are activated next
                //
                // The epoch of std::u64::MAX - 1 is a placeholder and is expected to be reduced in
                // a future hard fork.
                Some(vec![
                    solana_config_program!(),
                    solana_storage_program!(),
                    solana_vest_program!(),
                ])
            } else if epoch == std::u64::MAX {
                // Finally 3rd party BPF programs are available
                //
                // The epoch of std::u64::MAX is a placeholder and is expected to be reduced in a
                // future hard fork.
                Some(vec![solana_bpf_loader_program!()])
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
            bank.set_inflation(inflation);
        }
        if let Some(new_programs) = get_programs(operating_mode, bank.epoch()) {
            for (name, program_id) in new_programs.iter() {
                info!("Registering {} at {}", name, program_id);
                bank.register_native_instruction_processor(name, program_id);
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::nonce_program::solana_nonce_program;
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
            11
        );
        assert_eq!(get_programs(OperatingMode::Development, 1), None);
    }

    #[test]
    fn test_softlaunch_inflation() {
        assert_eq!(
            get_inflation(OperatingMode::SoftLaunch, 0).unwrap(),
            Inflation::new_disabled()
        );
        assert_eq!(get_inflation(OperatingMode::SoftLaunch, 1), None);
        assert_eq!(
            get_inflation(OperatingMode::SoftLaunch, std::u64::MAX).unwrap(),
            Inflation::default()
        );
    }

    #[test]
    fn test_softlaunch_programs() {
        assert_eq!(
            get_programs(OperatingMode::SoftLaunch, 0),
            Some(vec![
                solana_nonce_program(),
                solana_stake_program!(),
                solana_system_program(),
                solana_vote_program!(),
            ])
        );
        assert_eq!(get_programs(OperatingMode::SoftLaunch, 1), None);
        assert!(get_programs(OperatingMode::SoftLaunch, std::u64::MAX - 1).is_some());
        assert!(get_programs(OperatingMode::SoftLaunch, std::u64::MAX).is_some());
    }
}
