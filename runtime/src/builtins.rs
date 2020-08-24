use crate::{
    bank::{Builtin, Entrypoint},
    system_instruction_processor,
};
use solana_sdk::{clock::Epoch, genesis_config::OperatingMode, system_program};

use log::*;

/// The entire set of available builtin programs that should be active at the given (operating_mode, epoch)
pub fn get_builtins(operating_mode: OperatingMode, epoch: Epoch) -> Vec<Builtin> {
    trace!("get_builtins: {:?}, {:?}", operating_mode, epoch);
    let mut builtins = vec![];

    builtins.extend(vec![
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
    ]);

    #[cfg(test)]
    if operating_mode == OperatingMode::Development && epoch >= 2 {
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
        builtins.extend(vec![Builtin::new(
            "mock",
            program_id,
            Entrypoint::Program(mock_ix_processor),
        )]);
    }

    builtins
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bank::Bank;
    use solana_sdk::{genesis_config::create_genesis_config, pubkey::Pubkey};

    use std::str::FromStr;
    use std::sync::Arc;

    #[test]
    fn test_get_builtins() {
        let (genesis_config, _mint_keypair) = create_genesis_config(100_000);
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
        let warped_bank = Arc::new(Bank::new_from_parent(
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
                Pubkey::from_str("7saCc6X5a2syoYANA5oUUnPZLcLMfKoSjiDhFU5fbpoK").unwrap(),
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
                Pubkey::from_str("7saCc6X5a2syoYANA5oUUnPZLcLMfKoSjiDhFU5fbpoK").unwrap(),
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
                Pubkey::from_str("7saCc6X5a2syoYANA5oUUnPZLcLMfKoSjiDhFU5fbpoK").unwrap(),
            ]
        );
    }
}
