#![cfg(feature = "sbf_rust")]

use {
    solana_runtime::{
        bank::Bank,
        bank_client::BankClient,
        epoch_stakes::EpochStakes,
        genesis_utils::{
            create_genesis_config_with_vote_accounts, GenesisConfigInfo, ValidatorVoteKeypairs,
        },
        loader_utils::load_upgradeable_program_and_advance_slot,
    },
    solana_sdk::{
        instruction::{AccountMeta, Instruction},
        message::Message,
        signature::{Keypair, Signer},
        transaction::{SanitizedTransaction, Transaction},
    },
    solana_vote::vote_account::VoteAccount,
    solana_vote_program::vote_state::create_account_with_authorized,
    std::collections::HashMap,
};

#[test]
fn test_syscall_get_epoch_stake() {
    solana_logger::setup();

    // Two vote accounts with stake.
    let stakes = vec![100_000_000, 500_000_000];
    let voting_keypairs = vec![
        ValidatorVoteKeypairs::new_rand(),
        ValidatorVoteKeypairs::new_rand(),
    ];
    let total_stake: u64 = stakes.iter().sum();

    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config_with_vote_accounts(1_000_000_000, &voting_keypairs, stakes.clone());

    let mut bank = Bank::new_for_tests(&genesis_config);

    // Intentionally overwrite the bank epoch with no stake, to ensure the
    // syscall gets the _current_ epoch stake based on the leader schedule
    // (N + 1).
    let epoch_stakes_epoch_0 = EpochStakes::new_for_tests(
        voting_keypairs
            .iter()
            .map(|keypair| {
                let node_id = keypair.node_keypair.pubkey();
                let authorized_voter = keypair.vote_keypair.pubkey();
                let vote_account = VoteAccount::try_from(create_account_with_authorized(
                    &node_id,
                    &authorized_voter,
                    &node_id,
                    0,
                    100,
                ))
                .unwrap();
                (authorized_voter, (0, vote_account)) // No stake.
            })
            .collect::<HashMap<_, _>>(),
        0, // Leader schedule epoch 0
    );
    bank.set_epoch_stakes_for_test(0, epoch_stakes_epoch_0);

    let (bank, bank_forks) = bank.wrap_with_bank_forks_for_tests();
    let mut bank_client = BankClient::new_shared(bank);

    let authority_keypair = Keypair::new();
    let (bank, program_id) = load_upgradeable_program_and_advance_slot(
        &mut bank_client,
        bank_forks.as_ref(),
        &mint_keypair,
        &authority_keypair,
        "solana_sbf_syscall_get_epoch_stake",
    );
    bank.freeze();

    let instruction = Instruction::new_with_bytes(
        program_id,
        &[],
        vec![
            AccountMeta::new_readonly(voting_keypairs[0].vote_keypair.pubkey(), false),
            AccountMeta::new_readonly(voting_keypairs[1].vote_keypair.pubkey(), false),
        ],
    );

    let blockhash = bank.last_blockhash();
    let message = Message::new(&[instruction], Some(&mint_keypair.pubkey()));
    let transaction = Transaction::new(&[&mint_keypair], message, blockhash);
    let sanitized_tx = SanitizedTransaction::from_transaction_for_tests(transaction);

    let result = bank.simulate_transaction(&sanitized_tx, false);

    assert!(result.result.is_ok());

    let return_data_le_bytes: [u8; 8] = result.return_data.unwrap().data[0..8].try_into().unwrap();
    let total_stake_from_return_data = u64::from_le_bytes(return_data_le_bytes);
    assert_eq!(total_stake_from_return_data, total_stake);
}
