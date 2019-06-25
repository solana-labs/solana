use solana_runtime::bank::Bank;
use solana_runtime::bank_client::BankClient;
use solana_runtime::genesis_utils::{create_genesis_block, GenesisBlockInfo};
use solana_sdk::account_utils::State;
use solana_sdk::client::SyncClient;
use solana_sdk::message::Message;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_stake_api::id;
use solana_stake_api::stake_instruction;
use solana_stake_api::stake_instruction::process_instruction;
use solana_stake_api::stake_state::StakeState;
use solana_vote_api::vote_instruction;
use std::sync::Arc;

#[test]
fn test_stake_account_delegate() {
    let staker_keypair = Keypair::new();
    let staker_pubkey = staker_keypair.pubkey();
    let vote_pubkey = Pubkey::new_rand();
    let node_pubkey = Pubkey::new_rand();

    let GenesisBlockInfo {
        genesis_block,
        mint_keypair,
        ..
    } = create_genesis_block(1000);
    let mut bank = Bank::new(&genesis_block);
    let mint_pubkey = mint_keypair.pubkey();
    bank.add_instruction_processor(id(), process_instruction);
    let bank = Arc::new(bank);
    let bank_client = BankClient::new_shared(&bank);

    // Create Vote Account
    let message = Message::new(vote_instruction::create_account(
        &mint_pubkey,
        &vote_pubkey,
        &node_pubkey,
        1,
        10,
    ));
    bank_client
        .send_message(&[&mint_keypair], message)
        .expect("failed to create vote account");

    // Create stake account and delegate to vote account
    let message = Message::new(stake_instruction::create_stake_account_and_delegate_stake(
        &mint_pubkey,
        &staker_pubkey,
        &vote_pubkey,
        200,
    ));
    bank_client
        .send_message(&[&mint_keypair, &staker_keypair], message)
        .expect("failed to create and delegate stake account");

    // Test that correct lamports are staked
    let account = bank.get_account(&staker_pubkey).expect("account not found");
    let stake_state = account.state().expect("couldn't unpack account data");
    if let StakeState::Stake(stake) = stake_state {
        assert_eq!(stake.stake, 200);
    } else {
        assert!(false, "wrong account type found")
    }

    // Test that we cannot withdraw staked lamports
    let message = Message::new_with_payer(
        vec![stake_instruction::withdraw(
            &staker_pubkey,
            &Pubkey::new_rand(),
            100,
        )],
        Some(&mint_pubkey),
    );
    assert!(bank_client
        .send_message(&[&mint_keypair, &staker_keypair], message)
        .is_err());

    // Test that lamports are still staked
    let account = bank.get_account(&staker_pubkey).expect("account not found");
    let stake_state = account.state().expect("couldn't unpack account data");
    if let StakeState::Stake(stake) = stake_state {
        assert_eq!(stake.stake, 200);
    } else {
        assert!(false, "wrong account type found")
    }

    // Deactivate the stake
    let message = Message::new_with_payer(
        vec![stake_instruction::deactivate_stake(&staker_pubkey)],
        Some(&mint_pubkey),
    );
    assert!(bank_client
        .send_message(&[&mint_keypair, &staker_keypair], message)
        .is_ok());

    // Test that we cannot withdraw staked lamports due to cooldown period
    let message = Message::new_with_payer(
        vec![stake_instruction::withdraw(
            &staker_pubkey,
            &Pubkey::new_rand(),
            100,
        )],
        Some(&mint_pubkey),
    );
    assert!(bank_client
        .send_message(&[&mint_keypair, &staker_keypair], message)
        .is_err());

    // Create a new bank at later epoch (to account for cooldown of stake)
    let mut bank = Bank::new_from_parent(
        &bank,
        &Pubkey::default(),
        genesis_block.slots_per_epoch * 8 + bank.slot(),
    );
    bank.add_instruction_processor(id(), process_instruction);
    let bank = Arc::new(bank);
    let bank_client = BankClient::new_shared(&bank);

    // Test that we can withdraw now
    let message = Message::new_with_payer(
        vec![stake_instruction::withdraw(
            &staker_pubkey,
            &Pubkey::new_rand(),
            100,
        )],
        Some(&mint_pubkey),
    );
    assert!(bank_client
        .send_message(&[&mint_keypair, &staker_keypair], message)
        .is_ok());

    // Test that balance and stake is updated correctly
    let account = bank.get_account(&staker_pubkey).expect("account not found");
    let stake_state = account.state().expect("couldn't unpack account data");
    if let StakeState::Stake(stake) = stake_state {
        assert_eq!(account.lamports, 100);
        assert_eq!(stake.stake, 100);
    } else {
        assert!(false, "wrong account type found")
    }
}
