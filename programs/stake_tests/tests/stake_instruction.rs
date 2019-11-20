use assert_matches::assert_matches;
use solana_runtime::{
    bank::Bank,
    bank_client::BankClient,
    genesis_utils::{create_genesis_config_with_leader, GenesisConfigInfo},
};
use solana_sdk::{
    account_utils::State,
    client::SyncClient,
    message::Message,
    pubkey::Pubkey,
    signature::{Keypair, KeypairUtil},
    sysvar::{self, rewards::Rewards, stake_history::StakeHistory, Sysvar},
};
use solana_stake_api::{
    stake_instruction::{self},
    stake_state::{self, StakeState},
};
use solana_vote_api::{
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
            .send_message(&[&mint_keypair, &vote_keypair], message)
            .is_ok());
    }
    bank
}

fn warmed_up(bank: &Bank, stake_pubkey: &Pubkey) -> bool {
    let stake = StakeState::stake_from(&bank.get_account(stake_pubkey).unwrap()).unwrap();

    stake.stake
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
            commission: std::u8::MAX / 2,
        },
        10,
    ));
    bank_client
        .send_message(&[&mint_keypair, &vote_keypair], message)
        .expect("failed to create vote account");

    let authorized = stake_state::Authorized::auto(&stake_pubkey);
    // Create stake account and delegate to vote account
    let message = Message::new(stake_instruction::create_stake_account_and_delegate_stake(
        &mint_pubkey,
        &stake_pubkey,
        &vote_pubkey,
        &authorized,
        1_000_000,
    ));
    bank_client
        .send_message(&[&mint_keypair, &stake_keypair], message)
        .expect("failed to create and delegate stake account");

    // Test that correct lamports are staked
    let account = bank.get_account(&stake_pubkey).expect("account not found");
    let stake_state = account.state().expect("couldn't unpack account data");
    if let StakeState::Stake(_meta, stake) = stake_state {
        assert_eq!(stake.stake, 1_000_000);
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
        assert_eq!(stake.stake, 1_000_000);
    } else {
        assert!(false, "wrong account type found")
    }

    // Reward redemption
    // Submit enough votes to generate rewards
    bank = fill_epoch_with_votes(&bank, &vote_keypair, &mint_keypair);

    // Test that votes and credits are there
    let account = bank.get_account(&vote_pubkey).expect("account not found");
    let vote_state: VoteState = account.state().expect("couldn't unpack account data");

    // 1 less vote, as the first vote should have cleared the lockout
    assert_eq!(vote_state.votes.len(), 31);
    assert_eq!(vote_state.credits(), 1);
    bank = fill_epoch_with_votes(&bank, &vote_keypair, &mint_keypair);

    loop {
        if warmed_up(&bank, &stake_pubkey) {
            break;
        }
        // Cycle thru banks until we're fully warmed up
        bank = next_epoch(&bank);
    }

    // Test that rewards are there
    let rewards_account = bank
        .get_account(&sysvar::rewards::id())
        .expect("account not found");
    assert_matches!(Rewards::from_account(&rewards_account), Some(_));

    let pre_staked = get_staked(&bank, &stake_pubkey);

    // Redeem the credit
    let bank_client = BankClient::new_shared(&bank);
    let message = Message::new_with_payer(
        vec![stake_instruction::redeem_vote_credits(
            &stake_pubkey,
            &vote_pubkey,
        )],
        Some(&mint_pubkey),
    );
    assert_matches!(bank_client.send_message(&[&mint_keypair], message), Ok(_));

    // Test that balance increased, and that the balance got staked
    let staked = get_staked(&bank, &stake_pubkey);
    let lamports = bank.get_balance(&stake_pubkey);
    assert!(staked > pre_staked);
    assert!(lamports > 1_000_000);

    // split the stake
    let split_stake_keypair = Keypair::new();
    let split_stake_pubkey = split_stake_keypair.pubkey();
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
