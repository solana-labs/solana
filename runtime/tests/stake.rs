use solana_runtime::{
    bank::Bank,
    bank_client::BankClient,
    genesis_utils::{create_genesis_config_with_leader, GenesisConfigInfo},
};
use solana_sdk::{
    account_utils::StateMut,
    client::SyncClient,
    message::Message,
    pubkey::Pubkey,
    signature::{Keypair, KeypairUtil},
    system_instruction::create_address_with_seed,
    sysvar::{self, stake_history::StakeHistory, Sysvar},
};
use solana_stake_program::{
    stake_instruction::{self},
    stake_state::{self, StakeState},
};
use solana_vote_program::{
    vote_instruction,
    vote_state::{Vote, VoteInit, VoteState},
};
use std::sync::Arc;

fn next_epoch(bank: &Arc<Bank>) -> Arc<Bank> {
    bank.squash();

    Arc::new(Bank::new_from_parent(
        &bank,
        &Pubkey::default(),
        bank.get_slots_in_epoch(bank.epoch()) + bank.slot(),
    ))
}

fn fill_epoch_with_votes(
    bank: &Arc<Bank>,
    vote_keypair: &Keypair,
    mint_keypair: &Keypair,
) -> Arc<Bank> {
    let mint_pubkey = mint_keypair.pubkey();
    let vote_pubkey = vote_keypair.pubkey();
    let old_epoch = bank.epoch();
    let mut bank = bank.clone();
    while bank.epoch() != old_epoch + 1 {
        bank.squash();
        bank = Arc::new(Bank::new_from_parent(
            &bank,
            &Pubkey::default(),
            1 + bank.slot(),
        ));

        let bank_client = BankClient::new_shared(&bank);
        let parent = bank.parent().unwrap();

        let message = Message::new_with_payer(
            vec![vote_instruction::vote(
                &vote_pubkey,
                &vote_pubkey,
                Vote::new(vec![parent.slot() as u64], parent.hash()),
            )],
            Some(&mint_pubkey),
        );
        assert!(bank_client
            .send_message(&[mint_keypair, vote_keypair], message)
            .is_ok());
    }
    bank
}

fn warmed_up(bank: &Bank, stake_pubkey: &Pubkey) -> bool {
    let stake = StakeState::stake_from(&bank.get_account(stake_pubkey).unwrap()).unwrap();

    stake.delegation.stake
        == stake.stake(
            bank.epoch(),
            Some(
                &StakeHistory::from_account(
                    &bank.get_account(&sysvar::stake_history::id()).unwrap(),
                )
                .unwrap(),
            ),
        )
}

fn get_staked(bank: &Bank, stake_pubkey: &Pubkey) -> u64 {
    StakeState::stake_from(&bank.get_account(stake_pubkey).unwrap())
        .unwrap()
        .stake(
            bank.epoch(),
            Some(
                &StakeHistory::from_account(
                    &bank.get_account(&sysvar::stake_history::id()).unwrap(),
                )
                .unwrap(),
            ),
        )
}

#[test]
fn test_stake_create_and_split_single_signature() {
    solana_logger::setup();

    let GenesisConfigInfo {
        mut genesis_config,
        mint_keypair: staker_keypair,
        ..
    } = create_genesis_config_with_leader(100_000_000_000, &Pubkey::new_rand(), 1_000_000);
    genesis_config
        .native_instruction_processors
        .push(solana_stake_program::solana_stake_program!());

    let staker_pubkey = staker_keypair.pubkey();

    let bank_client = BankClient::new_shared(&Arc::new(Bank::new(&genesis_config)));

    let stake_address =
        create_address_with_seed(&staker_pubkey, "stake", &solana_stake_program::id()).unwrap();

    let authorized = stake_state::Authorized::auto(&staker_pubkey);

    let lamports = 1_000_000;

    // Create stake account with seed
    let message = Message::new(stake_instruction::create_account_with_seed(
        &staker_pubkey, // from
        &stake_address, // to
        &staker_pubkey, // base
        "stake",        // seed
        &authorized,
        &stake_state::Lockup::default(),
        lamports,
    ));

    // only one signature required
    bank_client
        .send_message(&[&staker_keypair], message)
        .expect("failed to create and delegate stake account");

    // split the stake
    let split_stake_address =
        create_address_with_seed(&staker_pubkey, "split_stake", &solana_stake_program::id())
            .unwrap();
    // Test split
    let message = Message::new(stake_instruction::split_with_seed(
        &stake_address, // original
        &staker_pubkey, // authorized
        lamports / 2,
        &split_stake_address, // new address
        &staker_pubkey,       // base
        "split_stake",        // seed
    ));

    assert!(bank_client
        .send_message(&[&staker_keypair], message)
        .is_ok());

    // w00t!
}

#[test]
fn test_stake_account_lifetime() {
    let stake_keypair = Keypair::new();
    let stake_pubkey = stake_keypair.pubkey();
    let vote_keypair = Keypair::new();
    let vote_pubkey = vote_keypair.pubkey();
    let node_pubkey = Pubkey::new_rand();

    let GenesisConfigInfo {
        mut genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config_with_leader(100_000_000_000, &Pubkey::new_rand(), 1_000_000);
    genesis_config
        .native_instruction_processors
        .push(solana_stake_program::solana_stake_program!());
    let bank = Bank::new(&genesis_config);
    let mint_pubkey = mint_keypair.pubkey();
    let mut bank = Arc::new(bank);
    let bank_client = BankClient::new_shared(&bank);

    // Create Vote Account
    let message = Message::new(vote_instruction::create_account(
        &mint_pubkey,
        &vote_pubkey,
        &VoteInit {
            node_pubkey,
            authorized_voter: vote_pubkey,
            authorized_withdrawer: vote_pubkey,
            commission: 50,
        },
        10,
    ));
    bank_client
        .send_message(&[&mint_keypair, &vote_keypair], message)
        .expect("failed to create vote account");

    let authorized = stake_state::Authorized::auto(&stake_pubkey);
    // Create stake account and delegate to vote account
    let message = Message::new(stake_instruction::create_account_and_delegate_stake(
        &mint_pubkey,
        &stake_pubkey,
        &vote_pubkey,
        &authorized,
        &stake_state::Lockup::default(),
        1_000_000,
    ));
    bank_client
        .send_message(&[&mint_keypair, &stake_keypair], message)
        .expect("failed to create and delegate stake account");

    // Test that correct lamports are staked
    let account = bank.get_account(&stake_pubkey).expect("account not found");
    let stake_state = account.state().expect("couldn't unpack account data");
    if let StakeState::Stake(_meta, stake) = stake_state {
        assert_eq!(stake.delegation.stake, 1_000_000);
    } else {
        assert!(false, "wrong account type found")
    }

    // Test that we cannot withdraw anything until deactivation
    let message = Message::new_with_payer(
        vec![stake_instruction::withdraw(
            &stake_pubkey,
            &stake_pubkey,
            &Pubkey::new_rand(),
            1,
        )],
        Some(&mint_pubkey),
    );
    assert!(bank_client
        .send_message(&[&mint_keypair, &stake_keypair], message)
        .is_err());

    // Test that lamports are still staked
    let account = bank.get_account(&stake_pubkey).expect("account not found");
    let stake_state = account.state().expect("couldn't unpack account data");
    if let StakeState::Stake(_meta, stake) = stake_state {
        assert_eq!(stake.delegation.stake, 1_000_000);
    } else {
        assert!(false, "wrong account type found")
    }

    loop {
        if warmed_up(&bank, &stake_pubkey) {
            break;
        }
        // Cycle thru banks until we're fully warmed up
        bank = next_epoch(&bank);
    }

    // Reward redemption
    // Submit enough votes to generate rewards
    bank = fill_epoch_with_votes(&bank, &vote_keypair, &mint_keypair);

    // Test that votes and credits are there
    let account = bank.get_account(&vote_pubkey).expect("account not found");
    let vote_state: VoteState = account.state().expect("couldn't unpack account data");

    // 1 less vote, as the first vote should have cleared the lockout
    assert_eq!(vote_state.votes.len(), 31);
    // one vote per slot, might be more slots than 32 in the epoch
    assert!(vote_state.credits() >= 1);
    bank = fill_epoch_with_votes(&bank, &vote_keypair, &mint_keypair);

    let pre_staked = get_staked(&bank, &stake_pubkey);

    // next epoch bank should pay rewards
    bank = next_epoch(&bank);

    // Test that balance increased, and that the balance got staked
    let staked = get_staked(&bank, &stake_pubkey);
    let lamports = bank.get_balance(&stake_pubkey);
    assert!(staked > pre_staked);
    assert!(lamports > 1_000_000);

    // split the stake
    let split_stake_keypair = Keypair::new();
    let split_stake_pubkey = split_stake_keypair.pubkey();

    let bank_client = BankClient::new_shared(&bank);
    // Test split
    let message = Message::new_with_payer(
        stake_instruction::split(
            &stake_pubkey,
            &stake_pubkey,
            lamports / 2,
            &split_stake_pubkey,
        ),
        Some(&mint_pubkey),
    );
    assert!(bank_client
        .send_message(
            &[&mint_keypair, &stake_keypair, &split_stake_keypair],
            message
        )
        .is_ok());

    // Deactivate the split
    let message = Message::new_with_payer(
        vec![stake_instruction::deactivate_stake(
            &split_stake_pubkey,
            &stake_pubkey,
        )],
        Some(&mint_pubkey),
    );
    assert!(bank_client
        .send_message(&[&mint_keypair, &stake_keypair], message)
        .is_ok());

    let split_staked = get_staked(&bank, &split_stake_pubkey);

    // Test that we cannot withdraw above what's staked
    let message = Message::new_with_payer(
        vec![stake_instruction::withdraw(
            &split_stake_pubkey,
            &stake_pubkey,
            &Pubkey::new_rand(),
            lamports / 2 - split_staked + 1,
        )],
        Some(&mint_pubkey),
    );
    assert!(bank_client
        .send_message(&[&mint_keypair, &stake_keypair], message)
        .is_err());

    let mut bank = next_epoch(&bank);
    let bank_client = BankClient::new_shared(&bank);

    // assert we're still cooling down
    let split_staked = get_staked(&bank, &split_stake_pubkey);
    assert!(split_staked > 0);

    // withdrawal in cooldown
    let message = Message::new_with_payer(
        vec![stake_instruction::withdraw(
            &split_stake_pubkey,
            &stake_pubkey,
            &Pubkey::new_rand(),
            lamports / 2,
        )],
        Some(&mint_pubkey),
    );

    assert!(bank_client
        .send_message(&[&mint_keypair, &stake_keypair], message)
        .is_err());

    // but we can withdraw unstaked
    let message = Message::new_with_payer(
        vec![stake_instruction::withdraw(
            &split_stake_pubkey,
            &stake_pubkey,
            &Pubkey::new_rand(),
            lamports / 2 - split_staked,
        )],
        Some(&mint_pubkey),
    );
    // assert we can withdraw unstaked tokens
    assert!(bank_client
        .send_message(&[&mint_keypair, &stake_keypair], message)
        .is_ok());

    // finish cooldown
    loop {
        if get_staked(&bank, &split_stake_pubkey) == 0 {
            break;
        }
        bank = next_epoch(&bank);
    }
    let bank_client = BankClient::new_shared(&bank);

    // Test that we can withdraw everything else out of the split
    let message = Message::new_with_payer(
        vec![stake_instruction::withdraw(
            &split_stake_pubkey,
            &stake_pubkey,
            &Pubkey::new_rand(),
            split_staked,
        )],
        Some(&mint_pubkey),
    );
    assert!(bank_client
        .send_message(&[&mint_keypair, &stake_keypair], message)
        .is_ok());

    // verify all the math sums to zero
    assert_eq!(bank.get_balance(&split_stake_pubkey), 0);
    assert_eq!(bank.get_balance(&stake_pubkey), lamports - lamports / 2);
}

#[test]
fn test_create_stake_account_from_seed() {
    let vote_keypair = Keypair::new();
    let vote_pubkey = vote_keypair.pubkey();
    let node_pubkey = Pubkey::new_rand();

    let GenesisConfigInfo {
        mut genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config_with_leader(100_000_000_000, &Pubkey::new_rand(), 1_000_000);
    genesis_config
        .native_instruction_processors
        .push(solana_stake_program::solana_stake_program!());
    let bank = Bank::new(&genesis_config);
    let mint_pubkey = mint_keypair.pubkey();
    let bank = Arc::new(bank);
    let bank_client = BankClient::new_shared(&bank);

    let seed = "test-string";
    let stake_pubkey =
        create_address_with_seed(&mint_pubkey, seed, &solana_stake_program::id()).unwrap();

    // Create Vote Account
    let message = Message::new(vote_instruction::create_account(
        &mint_pubkey,
        &vote_pubkey,
        &VoteInit {
            node_pubkey,
            authorized_voter: vote_pubkey,
            authorized_withdrawer: vote_pubkey,
            commission: 50,
        },
        10,
    ));
    bank_client
        .send_message(&[&mint_keypair, &vote_keypair], message)
        .expect("failed to create vote account");

    let authorized = stake_state::Authorized::auto(&mint_pubkey);
    // Create stake account and delegate to vote account
    let message = Message::new(
        stake_instruction::create_account_with_seed_and_delegate_stake(
            &mint_pubkey,
            &stake_pubkey,
            &mint_pubkey,
            seed,
            &vote_pubkey,
            &authorized,
            &stake_state::Lockup::default(),
            1_000_000,
        ),
    );
    bank_client
        .send_message(&[&mint_keypair], message)
        .expect("failed to create and delegate stake account");

    // Test that correct lamports are staked
    let account = bank.get_account(&stake_pubkey).expect("account not found");
    let stake_state = account.state().expect("couldn't unpack account data");
    if let StakeState::Stake(_meta, stake) = stake_state {
        assert_eq!(stake.delegation.stake, 1_000_000);
    } else {
        assert!(false, "wrong account type found")
    }
}
