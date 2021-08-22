use log::*;
use solana_sdk::{
    client::SyncClient,
    commitment_config::CommitmentConfig,
    message::Message,
    pubkey::Pubkey,
    signature::Signer,
    signer::keypair::Keypair,
    stake::instruction as stake_instruction,
    stake::state::{Authorized, Lockup},
};
use solana_stake_program::stake_state;
use solana_vote_program::{
    vote_instruction,
    vote_state::{VoteInit, VoteState},
};
use std::io::{Error, ErrorKind, Result};

pub fn setup_vote_and_stake_accounts<T: SyncClient>(
    client: &T,
    vote_account: &Keypair,
    from_account: &Keypair,
    amount: u64,
    num_stake_accounts_per_vote_account: usize,
) -> Result<()> {
    let vote_account_pubkey = vote_account.pubkey();
    let node_pubkey = from_account.pubkey();
    info!(
        "setup_vote_and_stake_accounts: {}, {}",
        node_pubkey, vote_account_pubkey
    );

    let mut stake_account_pubkey = Pubkey::default();
    // Create the vote account if necessary
    if client
        .get_balance_with_commitment(&vote_account_pubkey, CommitmentConfig::processed())
        .unwrap_or(0)
        == 0
    {
        // 1) Create vote account

        let instructions = vote_instruction::create_account(
            &from_account.pubkey(),
            &vote_account_pubkey,
            &VoteInit {
                node_pubkey,
                authorized_voter: vote_account_pubkey,
                authorized_withdrawer: vote_account_pubkey,
                commission: 0,
            },
            amount,
        );
        let message = Message::new(&instructions, Some(&from_account.pubkey()));
        client
            .send_and_confirm_message(&[from_account, vote_account], message)
            .expect("fund vote");
        assert_eq!(
            client
                .get_balance_with_commitment(&vote_account_pubkey, CommitmentConfig::processed(),)
                .expect("get balance"),
            amount
        );

        for _ in 0..num_stake_accounts_per_vote_account {
            let stake_account_keypair = Keypair::new();
            stake_account_pubkey = stake_account_keypair.pubkey();

            let instructions = stake_instruction::create_account_and_delegate_stake(
                &from_account.pubkey(),
                &stake_account_pubkey,
                &vote_account_pubkey,
                &Authorized::auto(&stake_account_pubkey),
                &Lockup::default(),
                amount,
            );
            let message = Message::new(&instructions, Some(&from_account.pubkey()));
            client
                .send_and_confirm_message(&[from_account, &stake_account_keypair], message)
                .expect("delegate stake");
            assert_eq!(
                client
                    .get_balance_with_commitment(
                        &stake_account_pubkey,
                        CommitmentConfig::processed(),
                    )
                    .expect("get balance"),
                amount
            );
        }
    } else {
        warn!(
            "{} vote_account already has a balance?!?",
            vote_account_pubkey
        );
    }
    info!("Checking for vote account registration of {}", node_pubkey);
    match (
        client.get_account_with_commitment(&stake_account_pubkey, CommitmentConfig::processed()),
        client.get_account_with_commitment(&vote_account_pubkey, CommitmentConfig::processed()),
    ) {
        (Ok(Some(stake_account)), Ok(Some(vote_account))) => {
            match (
                stake_state::stake_from(&stake_account),
                VoteState::from(&vote_account),
            ) {
                (Some(stake_state), Some(vote_state)) => {
                    if stake_state.delegation.voter_pubkey != vote_account_pubkey
                        || stake_state.delegation.stake != amount
                    {
                        Err(Error::new(ErrorKind::Other, "invalid stake account state"))
                    } else if vote_state.node_pubkey != node_pubkey {
                        Err(Error::new(ErrorKind::Other, "invalid vote account state"))
                    } else {
                        info!("node {} {:?} {:?}", node_pubkey, stake_state, vote_state);

                        Ok(())
                    }
                }
                (None, _) => Err(Error::new(ErrorKind::Other, "invalid stake account data")),
                (_, None) => Err(Error::new(ErrorKind::Other, "invalid vote account data")),
            }
        }
        (Ok(None), _) | (Err(_), _) => Err(Error::new(
            ErrorKind::Other,
            "unable to retrieve stake account data",
        )),
        (_, Ok(None)) | (_, Err(_)) => Err(Error::new(
            ErrorKind::Other,
            "unable to retrieve vote account data",
        )),
    }
}
