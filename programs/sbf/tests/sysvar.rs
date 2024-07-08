use {
    solana_runtime::{
        bank::Bank,
        bank_client::BankClient,
        genesis_utils::{create_genesis_config, GenesisConfigInfo},
        loader_utils::load_upgradeable_program_and_advance_slot,
    },
    solana_sdk::{
        feature_set::disable_fees_sysvar,
        instruction::{AccountMeta, Instruction},
        message::Message,
        pubkey::Pubkey,
        signature::{Keypair, Signer},
        sysvar::{
            clock, epoch_rewards, epoch_schedule, instructions, recent_blockhashes, rent,
            slot_hashes, slot_history, stake_history,
        },
        transaction::{SanitizedTransaction, Transaction},
    },
};

#[test]
#[cfg(feature = "sbf_rust")]
fn test_sysvar_syscalls() {
    solana_logger::setup();

    let GenesisConfigInfo {
        mut genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config(50);
    genesis_config.accounts.remove(&disable_fees_sysvar::id());
    let bank = Bank::new_for_tests(&genesis_config);
    let epoch_rewards = epoch_rewards::EpochRewards {
        distribution_starting_block_height: 42,
        total_rewards: 100,
        distributed_rewards: 50,
        active: true,
        ..epoch_rewards::EpochRewards::default()
    };
    bank.set_sysvar_for_tests(&epoch_rewards);
    let (bank, bank_forks) = bank.wrap_with_bank_forks_for_tests();
    let mut bank_client = BankClient::new_shared(bank);
    let authority_keypair = Keypair::new();
    let (bank, program_id) = load_upgradeable_program_and_advance_slot(
        &mut bank_client,
        bank_forks.as_ref(),
        &mint_keypair,
        &authority_keypair,
        "solana_sbf_rust_sysvar",
    );
    bank.freeze();

    for instruction_data in &[0u8, 1u8] {
        let instruction = Instruction::new_with_bincode(
            program_id,
            &[instruction_data],
            vec![
                AccountMeta::new(mint_keypair.pubkey(), true),
                AccountMeta::new(Pubkey::new_unique(), false),
                AccountMeta::new_readonly(clock::id(), false),
                AccountMeta::new_readonly(epoch_schedule::id(), false),
                AccountMeta::new_readonly(instructions::id(), false),
                #[allow(deprecated)]
                AccountMeta::new_readonly(recent_blockhashes::id(), false),
                AccountMeta::new_readonly(rent::id(), false),
                AccountMeta::new_readonly(slot_hashes::id(), false),
                AccountMeta::new_readonly(slot_history::id(), false),
                AccountMeta::new_readonly(stake_history::id(), false),
                AccountMeta::new_readonly(epoch_rewards::id(), false),
            ],
        );
        let blockhash = bank.last_blockhash();
        let message = Message::new(&[instruction], Some(&mint_keypair.pubkey()));
        let transaction = Transaction::new(&[&mint_keypair], message, blockhash);
        let sanitized_tx = SanitizedTransaction::from_transaction_for_tests(transaction);
        let result = bank.simulate_transaction(&sanitized_tx, false);
        assert!(result.result.is_ok());
    }
}
