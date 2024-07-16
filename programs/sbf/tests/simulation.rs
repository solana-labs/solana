#![cfg(feature = "sbf_rust")]

use {
    agave_validator::test_validator::*,
    solana_runtime::{
        bank::Bank,
        bank_client::BankClient,
        genesis_utils::{create_genesis_config, GenesisConfigInfo},
        loader_utils::load_upgradeable_program_and_advance_slot,
    },
    solana_sdk::{
        instruction::{AccountMeta, Instruction},
        message::Message,
        pubkey::Pubkey,
        signature::{Keypair, Signer},
        sysvar::{clock, slot_history},
        transaction::{SanitizedTransaction, Transaction},
    },
};

#[test]
fn test_no_panic_banks_client() {
    solana_logger::setup();

    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config(50);
    let (bank, bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);
    let mut bank_client = BankClient::new_shared(bank.clone());
    let authority_keypair = Keypair::new();
    let (bank, program_id) = load_upgradeable_program_and_advance_slot(
        &mut bank_client,
        bank_forks.as_ref(),
        &mint_keypair,
        &authority_keypair,
        "solana_sbf_rust_simulation",
    );
    bank.freeze();

    let instruction = Instruction::new_with_bincode(
        program_id,
        &[0u8; 0],
        vec![
            AccountMeta::new_readonly(slot_history::id(), false),
            AccountMeta::new_readonly(clock::id(), false),
        ],
    );
    let blockhash = bank.last_blockhash();
    let message = Message::new(&[instruction], Some(&mint_keypair.pubkey()));
    let transaction = Transaction::new(&[&mint_keypair], message, blockhash);
    let sanitized_tx = SanitizedTransaction::from_transaction_for_tests(transaction);
    let result = bank.simulate_transaction(&sanitized_tx, false);
    assert!(result.result.is_ok());
}

#[test]
fn test_no_panic_rpc_client() {
    solana_logger::setup();

    let program_id = Pubkey::new_unique();
    let (test_validator, payer) = TestValidatorGenesis::default()
        .add_program("solana_sbf_rust_simulation", program_id)
        .start();
    let rpc_client = test_validator.get_rpc_client();
    let blockhash = rpc_client.get_latest_blockhash().unwrap();

    let transaction = Transaction::new_signed_with_payer(
        &[Instruction {
            program_id,
            accounts: vec![
                AccountMeta::new_readonly(slot_history::id(), false),
                AccountMeta::new_readonly(clock::id(), false),
            ],
            data: vec![],
        }],
        Some(&payer.pubkey()),
        &[&payer],
        blockhash,
    );

    rpc_client
        .send_and_confirm_transaction(&transaction)
        .unwrap();
}
