use crate::{
    bank::{Builtin, Entrypoint},
    system_instruction_processor,
};
use solana_sdk::{
    clock::{Epoch, GENESIS_EPOCH},
    genesis_config::ClusterType,
    system_program,
};

use log::*;

/// The entire set of available builtin programs that should be active at the given cluster_type
pub fn get_builtins(cluster_type: ClusterType) -> Vec<(Builtin, Epoch)> {
    trace!("get_builtins: {:?}", cluster_type);
    let mut builtins = vec![];

    builtins.extend(
        vec![
            Builtin::new(
                "system_program",
                system_program::id(),
                Entrypoint::Program(system_instruction_processor::process_instruction),
            ),
            Builtin::new(
                "config_program",
                solana_config_program::id(),
                Entrypoint::Program(solana_config_program::config_processor::process_instruction),
            ),
            Builtin::new(
                "stake_program",
                solana_stake_program::id(),
                Entrypoint::Program(solana_stake_program::stake_instruction::process_instruction),
            ),
            Builtin::new(
                "vote_program",
                solana_vote_program::id(),
                Entrypoint::Program(solana_vote_program::vote_instruction::process_instruction),
            ),
        ]
        .into_iter()
        .map(|program| (program, GENESIS_EPOCH))
        .collect::<Vec<_>>(),
    );

    // repurpose Testnet for test_get_builtins because the Development is overloaded...
    #[cfg(test)]
    if cluster_type == ClusterType::Testnet {
        use solana_sdk::instruction::InstructionError;
        use solana_sdk::{account::KeyedAccount, pubkey::Pubkey};
        use std::str::FromStr;
        fn mock_ix_processor(
            _pubkey: &Pubkey,
            _ka: &[KeyedAccount],
            _data: &[u8],
        ) -> std::result::Result<(), InstructionError> {
            Err(InstructionError::Custom(42))
        }
        let program_id = Pubkey::from_str("7saCc6X5a2syoYANA5oUUnPZLcLMfKoSjiDhFU5fbpoK").unwrap();
        builtins.extend(vec![(
            Builtin::new("mock", program_id, Entrypoint::Program(mock_ix_processor)),
            2,
        )]);
    }

    let secp256k1_builtin = Builtin::new(
        "secp256k1_program",
        solana_sdk::secp256k1_program::id(),
        Entrypoint::Program(solana_secp256k1_program::process_instruction),
    );
    let secp_epoch = solana_sdk::secp256k1::is_enabled_epoch(cluster_type);
    builtins.push((secp256k1_builtin, secp_epoch));

    builtins
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bank::Bank;
    use solana_sdk::{
        genesis_config::{create_genesis_config, ClusterType},
        pubkey::Pubkey,
    };

    use std::collections::HashSet;
    use std::str::FromStr;
    use std::sync::Arc;

    fn do_test_uniqueness(builtins: Vec<(Builtin, Epoch)>) {
        let mut unique_ids = HashSet::new();
        let mut unique_names = HashSet::new();
        let mut prev_start_epoch = 0;
        for (builtin, next_start_epoch) in builtins {
            assert!(next_start_epoch >= prev_start_epoch);
            assert!(unique_ids.insert(builtin.name));
            assert!(unique_names.insert(builtin.id));
            prev_start_epoch = next_start_epoch;
        }
    }

    #[test]
    fn test_uniqueness() {
        do_test_uniqueness(get_builtins(ClusterType::Development));
        do_test_uniqueness(get_builtins(ClusterType::Devnet));
        do_test_uniqueness(get_builtins(ClusterType::Testnet));
        do_test_uniqueness(get_builtins(ClusterType::MainnetBeta));
    }

    #[test]
    fn test_get_builtins() {
        let mock_program_id =
            Pubkey::from_str("7saCc6X5a2syoYANA5oUUnPZLcLMfKoSjiDhFU5fbpoK").unwrap();

        let (mut genesis_config, _mint_keypair) = create_genesis_config(100_000);
        genesis_config.cluster_type = ClusterType::Testnet;
        let bank0 = Arc::new(Bank::new(&genesis_config));

        let restored_slot1 = genesis_config.epoch_schedule.get_first_slot_in_epoch(2);
        let bank1 = Arc::new(Bank::new_from_parent(
            &bank0,
            &Pubkey::default(),
            restored_slot1,
        ));

        let restored_slot2 = genesis_config.epoch_schedule.get_first_slot_in_epoch(3);
        let bank2 = Arc::new(Bank::new_from_parent(
            &bank1,
            &Pubkey::default(),
            restored_slot2,
        ));

        let warped_slot = genesis_config.epoch_schedule.get_first_slot_in_epoch(999);
        let warped_bank = Arc::new(Bank::warp_from_parent(
            &bank0,
            &Pubkey::default(),
            warped_slot,
        ));

        assert_eq!(bank0.slot(), 0);
        assert_eq!(
            bank0.builtin_program_ids(),
            vec![
                system_program::id(),
                solana_config_program::id(),
                solana_stake_program::id(),
                solana_vote_program::id(),
            ]
        );

        assert_eq!(bank1.slot(), restored_slot1);
        assert_eq!(
            bank1.builtin_program_ids(),
            vec![
                system_program::id(),
                solana_config_program::id(),
                solana_stake_program::id(),
                solana_vote_program::id(),
                mock_program_id,
            ]
        );

        assert_eq!(bank2.slot(), restored_slot2);
        assert_eq!(
            bank2.builtin_program_ids(),
            vec![
                system_program::id(),
                solana_config_program::id(),
                solana_stake_program::id(),
                solana_vote_program::id(),
                mock_program_id,
            ]
        );

        assert_eq!(warped_bank.slot(), warped_slot);
        assert_eq!(
            warped_bank.builtin_program_ids(),
            vec![
                system_program::id(),
                solana_config_program::id(),
                solana_stake_program::id(),
                solana_vote_program::id(),
                mock_program_id,
            ]
        );
    }
}
