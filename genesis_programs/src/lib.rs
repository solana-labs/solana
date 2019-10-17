use solana_sdk::{
    clock::Epoch, genesis_block::OperatingMode, pubkey::Pubkey,
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
#[cfg(feature = "move")]
#[macro_use]
extern crate solana_move_loader_program;
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

pub fn get(operating_mode: OperatingMode, epoch: Epoch) -> Option<Vec<(String, Pubkey)>> {
    match operating_mode {
        OperatingMode::Development => {
            if epoch == 0 {
                Some(vec![
                    // Enable all SoftLaunch programs
                    solana_system_program(),
                    solana_bpf_loader_program!(),
                    solana_config_program!(),
                    solana_stake_program!(),
                    solana_storage_program!(),
                    solana_vest_program!(),
                    solana_vote_program!(),
                    // Programs that are only available in Development mode
                    solana_budget_program!(),
                    solana_exchange_program!(),
                    #[cfg(feature = "move")]
                    solana_move_loader_program!(),
                ])
            } else {
                None
            }
        }
        OperatingMode::SoftLaunch => {
            if epoch == 0 {
                // Voting and Staking only at epoch 0
                Some(vec![solana_stake_program!(), solana_vote_program!()])
            } else if epoch == std::u64::MAX - 1 {
                // System program and Archivers are activated next
                //
                // The epoch of std::u64::MAX - 1 is a placeholder and is expected to be reduced in
                // a future hard fork.
                Some(vec![
                    solana_config_program!(),
                    solana_storage_program!(),
                    solana_system_program(),
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
        if let Some(new_programs) = get(operating_mode, bank.epoch()) {
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
    use std::collections::HashSet;

    #[test]
    fn test_id_uniqueness() {
        let mut unique = HashSet::new();
        let ids = get(OperatingMode::Development, 0).unwrap();
        assert!(ids.into_iter().all(move |id| unique.insert(id)));
    }

    #[test]
    fn test_development_programs() {
        assert_eq!(get(OperatingMode::Development, 0).unwrap().len(), 9);
        assert_eq!(get(OperatingMode::Development, 1), None);
    }

    #[test]
    fn test_softlaunch_programs() {
        assert_eq!(
            get(OperatingMode::SoftLaunch, 0),
            Some(vec![solana_stake_program!(), solana_vote_program!(),])
        );
        assert_eq!(get(OperatingMode::SoftLaunch, 1), None);
        assert!(get(OperatingMode::SoftLaunch, std::u64::MAX - 1).is_some());
        assert!(get(OperatingMode::SoftLaunch, std::u64::MAX).is_some());
    }
}
