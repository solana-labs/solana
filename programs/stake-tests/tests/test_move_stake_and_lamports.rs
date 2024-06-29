#![allow(clippy::arithmetic_side_effects)]

// NOTE this is temporarily ported from the bpf stake program repo so MoveStake and MoveLamports can be tested comprehensively
// in the future we will either port *all* instruction tests from bpf stake program and remove existing stakeinstruction tests
// or we will develop a text fixture system that allows fuzzing and obsoletes both existing test suites
// in other words the utility functions in this file should not be broken out into modules or used elsewhere

use {
    solana_program_test::*,
    solana_sdk::{
        account::Account as SolanaAccount,
        entrypoint::ProgramResult,
        feature_set::move_stake_and_move_lamports_ixs,
        instruction::Instruction,
        program_error::ProgramError,
        pubkey::Pubkey,
        signature::{Keypair, Signer},
        signers::Signers,
        stake::{
            self,
            instruction::{self as ixn, StakeError},
            program as stake_program,
            state::{Authorized, Lockup, Meta, Stake, StakeStateV2},
        },
        system_instruction, system_program,
        sysvar::{clock::Clock, stake_history::StakeHistory},
        transaction::{Transaction, TransactionError},
    },
    solana_vote_program::{
        self, vote_instruction,
        vote_state::{VoteInit, VoteState, VoteStateVersions},
    },
    test_case::test_matrix,
};

const NO_SIGNERS: &[Keypair] = &[];

fn program_test() -> ProgramTest {
    program_test_without_features(&[])
}

fn program_test_without_features(feature_ids: &[Pubkey]) -> ProgramTest {
    let mut program_test = ProgramTest::default();
    for feature_id in feature_ids {
        program_test.deactivate_feature(*feature_id);
    }

    program_test
}

#[derive(Debug, PartialEq)]
struct Accounts {
    validator: Keypair,
    voter: Keypair,
    withdrawer: Keypair,
    vote_account: Keypair,
}

impl Accounts {
    async fn initialize(&self, context: &mut ProgramTestContext) {
        let slot = context.genesis_config().epoch_schedule.first_normal_slot + 1;
        context.warp_to_slot(slot).unwrap();

        create_vote(
            context,
            &self.validator,
            &self.voter.pubkey(),
            &self.withdrawer.pubkey(),
            &self.vote_account,
        )
        .await;
    }
}

impl Default for Accounts {
    fn default() -> Self {
        Self {
            validator: Keypair::new(),
            voter: Keypair::new(),
            withdrawer: Keypair::new(),
            vote_account: Keypair::new(),
        }
    }
}

async fn create_vote(
    context: &mut ProgramTestContext,
    validator: &Keypair,
    voter: &Pubkey,
    withdrawer: &Pubkey,
    vote_account: &Keypair,
) {
    let rent = context.banks_client.get_rent().await.unwrap();
    let rent_voter = rent.minimum_balance(VoteState::size_of());

    let mut instructions = vec![system_instruction::create_account(
        &context.payer.pubkey(),
        &validator.pubkey(),
        rent.minimum_balance(0),
        0,
        &system_program::id(),
    )];
    instructions.append(&mut vote_instruction::create_account_with_config(
        &context.payer.pubkey(),
        &vote_account.pubkey(),
        &VoteInit {
            node_pubkey: validator.pubkey(),
            authorized_voter: *voter,
            authorized_withdrawer: *withdrawer,
            ..VoteInit::default()
        },
        rent_voter,
        vote_instruction::CreateVoteAccountConfig {
            space: VoteStateVersions::vote_state_size_of(true) as u64,
            ..Default::default()
        },
    ));

    let transaction = Transaction::new_signed_with_payer(
        &instructions,
        Some(&context.payer.pubkey()),
        &[validator, vote_account, &context.payer],
        context.last_blockhash,
    );

    // ignore errors for idempotency
    let _ = context.banks_client.process_transaction(transaction).await;
}

async fn transfer(context: &mut ProgramTestContext, recipient: &Pubkey, amount: u64) {
    let transaction = Transaction::new_signed_with_payer(
        &[system_instruction::transfer(
            &context.payer.pubkey(),
            recipient,
            amount,
        )],
        Some(&context.payer.pubkey()),
        &[&context.payer],
        context.last_blockhash,
    );
    context
        .banks_client
        .process_transaction(transaction)
        .await
        .unwrap();
}

async fn advance_epoch(context: &mut ProgramTestContext) {
    refresh_blockhash(context).await;

    let root_slot = context.banks_client.get_root_slot().await.unwrap();
    let slots_per_epoch = context.genesis_config().epoch_schedule.slots_per_epoch;
    context.warp_to_slot(root_slot + slots_per_epoch).unwrap();
}

async fn refresh_blockhash(context: &mut ProgramTestContext) {
    context.last_blockhash = context
        .banks_client
        .get_new_latest_blockhash(&context.last_blockhash)
        .await
        .unwrap();
}

async fn get_account(banks_client: &mut BanksClient, pubkey: &Pubkey) -> SolanaAccount {
    banks_client
        .get_account(*pubkey)
        .await
        .expect("client error")
        .expect("account not found")
}

async fn get_stake_account(
    banks_client: &mut BanksClient,
    pubkey: &Pubkey,
) -> (Meta, Option<Stake>, u64) {
    let stake_account = get_account(banks_client, pubkey).await;
    let lamports = stake_account.lamports;
    match bincode::deserialize::<StakeStateV2>(&stake_account.data).unwrap() {
        StakeStateV2::Initialized(meta) => (meta, None, lamports),
        StakeStateV2::Stake(meta, stake, _) => (meta, Some(stake), lamports),
        StakeStateV2::Uninitialized => panic!("panic: uninitialized"),
        _ => unimplemented!(),
    }
}

async fn get_stake_account_rent(banks_client: &mut BanksClient) -> u64 {
    let rent = banks_client.get_rent().await.unwrap();
    rent.minimum_balance(std::mem::size_of::<stake::state::StakeStateV2>())
}

async fn get_effective_stake(banks_client: &mut BanksClient, pubkey: &Pubkey) -> u64 {
    let clock = banks_client.get_sysvar::<Clock>().await.unwrap();
    let stake_history = banks_client.get_sysvar::<StakeHistory>().await.unwrap();
    let stake_account = get_account(banks_client, pubkey).await;
    match bincode::deserialize::<StakeStateV2>(&stake_account.data).unwrap() {
        StakeStateV2::Stake(_, stake, _) => {
            stake
                .delegation
                .stake_activating_and_deactivating(clock.epoch, &stake_history, Some(0))
                .effective
        }
        _ => 0,
    }
}

async fn get_minimum_delegation(context: &mut ProgramTestContext) -> u64 {
    let transaction = Transaction::new_signed_with_payer(
        &[stake::instruction::get_minimum_delegation()],
        Some(&context.payer.pubkey()),
        &[&context.payer],
        context.last_blockhash,
    );
    let mut data = context
        .banks_client
        .simulate_transaction(transaction)
        .await
        .unwrap()
        .simulation_details
        .unwrap()
        .return_data
        .unwrap()
        .data;
    data.resize(8, 0);

    data.try_into().map(u64::from_le_bytes).unwrap()
}

async fn create_blank_stake_account_from_keypair(
    context: &mut ProgramTestContext,
    stake: &Keypair,
) -> Pubkey {
    let lamports = get_stake_account_rent(&mut context.banks_client).await;

    let transaction = Transaction::new_signed_with_payer(
        &[system_instruction::create_account(
            &context.payer.pubkey(),
            &stake.pubkey(),
            lamports,
            StakeStateV2::size_of() as u64,
            &stake_program::id(),
        )],
        Some(&context.payer.pubkey()),
        &[&context.payer, stake],
        context.last_blockhash,
    );

    context
        .banks_client
        .process_transaction(transaction)
        .await
        .unwrap();

    stake.pubkey()
}

async fn process_instruction<T: Signers + ?Sized>(
    context: &mut ProgramTestContext,
    instruction: &Instruction,
    additional_signers: &T,
) -> ProgramResult {
    let mut transaction =
        Transaction::new_with_payer(&[instruction.clone()], Some(&context.payer.pubkey()));

    transaction.partial_sign(&[&context.payer], context.last_blockhash);
    transaction.sign(additional_signers, context.last_blockhash);

    match context.banks_client.process_transaction(transaction).await {
        Ok(_) => Ok(()),
        Err(e) => {
            // banks client error -> transaction error -> instruction error -> program error
            match e.unwrap() {
                TransactionError::InstructionError(_, e) => Err(e.try_into().unwrap()),
                TransactionError::InsufficientFundsForRent { .. } => {
                    Err(ProgramError::InsufficientFunds)
                }
                _ => panic!("couldnt convert {:?} to ProgramError", e),
            }
        }
    }
}

async fn test_instruction_with_missing_signers(
    context: &mut ProgramTestContext,
    instruction: &Instruction,
    additional_signers: &Vec<&Keypair>,
) {
    // remove every signer one by one and ensure we always fail
    for i in 0..instruction.accounts.len() {
        if instruction.accounts[i].is_signer {
            let mut instruction = instruction.clone();
            instruction.accounts[i].is_signer = false;
            let reduced_signers: Vec<_> = additional_signers
                .iter()
                .filter(|s| s.pubkey() != instruction.accounts[i].pubkey)
                .collect();

            let e = process_instruction(context, &instruction, &reduced_signers)
                .await
                .unwrap_err();
            assert_eq!(e, ProgramError::MissingRequiredSignature);
        }
    }

    // now make sure the instruction succeeds
    process_instruction(context, instruction, additional_signers)
        .await
        .unwrap();
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum StakeLifecycle {
    Uninitialized = 0,
    Initialized,
    Activating,
    Active,
    Deactivating,
    Deactive,
}
impl StakeLifecycle {
    // (stake, staker, withdrawer)
    async fn new_stake_account(
        self,
        context: &mut ProgramTestContext,
        vote_account: &Pubkey,
        staked_amount: u64,
    ) -> (Keypair, Keypair, Keypair) {
        let stake_keypair = Keypair::new();
        let staker_keypair = Keypair::new();
        let withdrawer_keypair = Keypair::new();

        self.new_stake_account_fully_specified(
            context,
            vote_account,
            staked_amount,
            &stake_keypair,
            &staker_keypair,
            &withdrawer_keypair,
            &Lockup::default(),
        )
        .await;

        (stake_keypair, staker_keypair, withdrawer_keypair)
    }

    #[allow(clippy::too_many_arguments)]
    async fn new_stake_account_fully_specified(
        self,
        context: &mut ProgramTestContext,
        vote_account: &Pubkey,
        staked_amount: u64,
        stake_keypair: &Keypair,
        staker_keypair: &Keypair,
        withdrawer_keypair: &Keypair,
        lockup: &Lockup,
    ) {
        let authorized = Authorized {
            staker: staker_keypair.pubkey(),
            withdrawer: withdrawer_keypair.pubkey(),
        };

        let stake = create_blank_stake_account_from_keypair(context, stake_keypair).await;
        if staked_amount > 0 {
            transfer(context, &stake, staked_amount).await;
        }

        if self >= StakeLifecycle::Initialized {
            let instruction = ixn::initialize(&stake, &authorized, lockup);
            process_instruction(context, &instruction, NO_SIGNERS)
                .await
                .unwrap();
        }

        if self >= StakeLifecycle::Activating {
            let instruction = ixn::delegate_stake(&stake, &staker_keypair.pubkey(), vote_account);
            process_instruction(context, &instruction, &vec![staker_keypair])
                .await
                .unwrap();
        }

        if self >= StakeLifecycle::Active {
            advance_epoch(context).await;
            assert_eq!(
                get_effective_stake(&mut context.banks_client, &stake).await,
                staked_amount,
            );
        }

        if self >= StakeLifecycle::Deactivating {
            let instruction = ixn::deactivate_stake(&stake, &staker_keypair.pubkey());
            process_instruction(context, &instruction, &vec![staker_keypair])
                .await
                .unwrap();
        }

        if self == StakeLifecycle::Deactive {
            advance_epoch(context).await;
            assert_eq!(
                get_effective_stake(&mut context.banks_client, &stake).await,
                0,
            );
        }
    }
}

#[test_matrix(
    [StakeLifecycle::Initialized, StakeLifecycle::Activating, StakeLifecycle::Active,
     StakeLifecycle::Deactivating, StakeLifecycle::Deactive],
    [StakeLifecycle::Initialized, StakeLifecycle::Activating, StakeLifecycle::Active,
     StakeLifecycle::Deactivating, StakeLifecycle::Deactive],
    [false, true],
    [false, true]
)]
#[tokio::test]
async fn test_move_stake(
    move_source_type: StakeLifecycle,
    move_dest_type: StakeLifecycle,
    full_move: bool,
    has_lockup: bool,
) {
    let mut context = program_test().start_with_context().await;
    let accounts = Accounts::default();
    accounts.initialize(&mut context).await;

    let rent_exempt_reserve = get_stake_account_rent(&mut context.banks_client).await;
    let minimum_delegation = get_minimum_delegation(&mut context).await;

    // source has 2x minimum so we can easily test an unfunded destination
    let source_staked_amount = minimum_delegation * 2;

    // this is the amount of *staked* lamports for test checks
    // destinations may have excess lamports but these are *never* activated by move
    let dest_staked_amount = if move_dest_type == StakeLifecycle::Active {
        minimum_delegation
    } else {
        0
    };

    // test with and without lockup. both of these cases pass, we test failures elsewhere
    let lockup = if has_lockup {
        let clock = context.banks_client.get_sysvar::<Clock>().await.unwrap();
        let lockup = Lockup {
            unix_timestamp: 0,
            epoch: clock.epoch + 100,
            custodian: Pubkey::new_unique(),
        };

        assert!(lockup.is_in_force(&clock, None));
        lockup
    } else {
        Lockup::default()
    };

    // we put an extra minimum in every account, unstaked, to test that no new lamports activate
    // name them here so our asserts are readable
    let source_excess = minimum_delegation;
    let dest_excess = minimum_delegation;

    let move_source_keypair = Keypair::new();
    let move_dest_keypair = Keypair::new();
    let staker_keypair = Keypair::new();
    let withdrawer_keypair = Keypair::new();

    // create source stake
    move_source_type
        .new_stake_account_fully_specified(
            &mut context,
            &accounts.vote_account.pubkey(),
            source_staked_amount,
            &move_source_keypair,
            &staker_keypair,
            &withdrawer_keypair,
            &lockup,
        )
        .await;
    let move_source = move_source_keypair.pubkey();
    let mut source_account = get_account(&mut context.banks_client, &move_source).await;
    let mut source_stake_state: StakeStateV2 = bincode::deserialize(&source_account.data).unwrap();

    // create dest stake with same authorities
    move_dest_type
        .new_stake_account_fully_specified(
            &mut context,
            &accounts.vote_account.pubkey(),
            minimum_delegation,
            &move_dest_keypair,
            &staker_keypair,
            &withdrawer_keypair,
            &lockup,
        )
        .await;
    let move_dest = move_dest_keypair.pubkey();

    // true up source epoch if transient
    if move_source_type == StakeLifecycle::Activating
        || move_source_type == StakeLifecycle::Deactivating
    {
        let clock = context.banks_client.get_sysvar::<Clock>().await.unwrap();
        if let StakeStateV2::Stake(_, ref mut stake, _) = &mut source_stake_state {
            match move_source_type {
                StakeLifecycle::Activating => stake.delegation.activation_epoch = clock.epoch,
                StakeLifecycle::Deactivating => stake.delegation.deactivation_epoch = clock.epoch,
                _ => (),
            }
        }

        source_account.data = bincode::serialize(&source_stake_state).unwrap();
        context.set_account(&move_source, &source_account.into());
    }

    // our inactive accounts have extra lamports, lets not let active feel left out
    if move_dest_type == StakeLifecycle::Active {
        transfer(&mut context, &move_dest, dest_excess).await;
    }

    // hey why not spread the love around to everyone
    transfer(&mut context, &move_source, source_excess).await;

    // alright first things first, clear out all the state failures
    match (move_source_type, move_dest_type) {
        // valid
        (StakeLifecycle::Active, StakeLifecycle::Initialized)
        | (StakeLifecycle::Active, StakeLifecycle::Active)
        | (StakeLifecycle::Active, StakeLifecycle::Deactive) => (),
        // invalid! get outta my test
        _ => {
            let instruction = ixn::move_stake(
                &move_source,
                &move_dest,
                &staker_keypair.pubkey(),
                if full_move {
                    source_staked_amount
                } else {
                    minimum_delegation
                },
            );

            // this is InvalidAccountData sometimes and Custom(5) sometimes but i dont care
            process_instruction(&mut context, &instruction, &vec![&staker_keypair])
                .await
                .unwrap_err();
            return;
        }
    }

    // source has 2x minimum (always 2 sol because these tests dont have featuresets)
    // so first for inactive accounts lets undershoot and fail for underfunded dest
    if move_dest_type != StakeLifecycle::Active {
        let instruction = ixn::move_stake(
            &move_source,
            &move_dest,
            &staker_keypair.pubkey(),
            minimum_delegation - 1,
        );

        let e = process_instruction(&mut context, &instruction, &vec![&staker_keypair])
            .await
            .unwrap_err();
        assert_eq!(e, ProgramError::InvalidArgument);
    }

    // now lets overshoot and fail for underfunded source
    let instruction = ixn::move_stake(
        &move_source,
        &move_dest,
        &staker_keypair.pubkey(),
        minimum_delegation + 1,
    );

    let e = process_instruction(&mut context, &instruction, &vec![&staker_keypair])
        .await
        .unwrap_err();
    assert_eq!(e, ProgramError::InvalidArgument);

    // now we do it juuust right
    let instruction = ixn::move_stake(
        &move_source,
        &move_dest,
        &staker_keypair.pubkey(),
        if full_move {
            source_staked_amount
        } else {
            minimum_delegation
        },
    );

    test_instruction_with_missing_signers(&mut context, &instruction, &vec![&staker_keypair]).await;

    if full_move {
        let (_, option_source_stake, source_lamports) =
            get_stake_account(&mut context.banks_client, &move_source).await;

        // source is deactivated and rent/excess stay behind
        assert!(option_source_stake.is_none());
        assert_eq!(source_lamports, source_excess + rent_exempt_reserve);

        let (_, Some(dest_stake), dest_lamports) =
            get_stake_account(&mut context.banks_client, &move_dest).await
        else {
            panic!("dest should be active")
        };
        let dest_effective_stake = get_effective_stake(&mut context.banks_client, &move_dest).await;

        // dest captured the entire source delegation, kept its rent/excess, didnt activate its excess
        assert_eq!(
            dest_stake.delegation.stake,
            source_staked_amount + dest_staked_amount
        );
        assert_eq!(dest_effective_stake, dest_stake.delegation.stake);
        assert_eq!(
            dest_lamports,
            dest_effective_stake + dest_excess + rent_exempt_reserve
        );
    } else {
        let (_, Some(source_stake), source_lamports) =
            get_stake_account(&mut context.banks_client, &move_source).await
        else {
            panic!("source should be active")
        };
        let source_effective_stake =
            get_effective_stake(&mut context.banks_client, &move_source).await;

        // half of source delegation moved over, excess stayed behind
        assert_eq!(source_stake.delegation.stake, source_staked_amount / 2);
        assert_eq!(source_effective_stake, source_stake.delegation.stake);
        assert_eq!(
            source_lamports,
            source_effective_stake + source_excess + rent_exempt_reserve
        );

        let (_, Some(dest_stake), dest_lamports) =
            get_stake_account(&mut context.banks_client, &move_dest).await
        else {
            panic!("dest should be active")
        };
        let dest_effective_stake = get_effective_stake(&mut context.banks_client, &move_dest).await;

        // dest mirrors our observations
        assert_eq!(
            dest_stake.delegation.stake,
            source_staked_amount / 2 + dest_staked_amount
        );
        assert_eq!(dest_effective_stake, dest_stake.delegation.stake);
        assert_eq!(
            dest_lamports,
            dest_effective_stake + dest_excess + rent_exempt_reserve
        );
    }
}

#[test_matrix(
    [StakeLifecycle::Initialized, StakeLifecycle::Activating, StakeLifecycle::Active,
     StakeLifecycle::Deactivating, StakeLifecycle::Deactive],
    [StakeLifecycle::Initialized, StakeLifecycle::Activating, StakeLifecycle::Active,
     StakeLifecycle::Deactivating, StakeLifecycle::Deactive],
    [false, true],
    [false, true]
)]
#[tokio::test]
async fn test_move_lamports(
    move_source_type: StakeLifecycle,
    move_dest_type: StakeLifecycle,
    different_votes: bool,
    has_lockup: bool,
) {
    let mut context = program_test().start_with_context().await;
    let accounts = Accounts::default();
    accounts.initialize(&mut context).await;

    let rent_exempt_reserve = get_stake_account_rent(&mut context.banks_client).await;
    let minimum_delegation = get_minimum_delegation(&mut context).await;

    // put minimum in both accounts if theyre active
    let source_staked_amount = if move_source_type == StakeLifecycle::Active {
        minimum_delegation
    } else {
        0
    };

    let dest_staked_amount = if move_dest_type == StakeLifecycle::Active {
        minimum_delegation
    } else {
        0
    };

    // test with and without lockup. both of these cases pass, we test failures elsewhere
    let lockup = if has_lockup {
        let clock = context.banks_client.get_sysvar::<Clock>().await.unwrap();
        let lockup = Lockup {
            unix_timestamp: 0,
            epoch: clock.epoch + 100,
            custodian: Pubkey::new_unique(),
        };

        assert!(lockup.is_in_force(&clock, None));
        lockup
    } else {
        Lockup::default()
    };

    // we put an extra minimum in every account, unstaked, to test moving them
    let source_excess = minimum_delegation;
    let dest_excess = minimum_delegation;

    let move_source_keypair = Keypair::new();
    let move_dest_keypair = Keypair::new();
    let staker_keypair = Keypair::new();
    let withdrawer_keypair = Keypair::new();

    // make a separate vote account if needed
    let dest_vote_account = if different_votes {
        let vote_account = Keypair::new();
        create_vote(
            &mut context,
            &Keypair::new(),
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            &vote_account,
        )
        .await;

        vote_account.pubkey()
    } else {
        accounts.vote_account.pubkey()
    };

    // create source stake
    move_source_type
        .new_stake_account_fully_specified(
            &mut context,
            &accounts.vote_account.pubkey(),
            minimum_delegation,
            &move_source_keypair,
            &staker_keypair,
            &withdrawer_keypair,
            &lockup,
        )
        .await;
    let move_source = move_source_keypair.pubkey();
    let mut source_account = get_account(&mut context.banks_client, &move_source).await;
    let mut source_stake_state: StakeStateV2 = bincode::deserialize(&source_account.data).unwrap();

    // create dest stake with same authorities
    move_dest_type
        .new_stake_account_fully_specified(
            &mut context,
            &dest_vote_account,
            minimum_delegation,
            &move_dest_keypair,
            &staker_keypair,
            &withdrawer_keypair,
            &lockup,
        )
        .await;
    let move_dest = move_dest_keypair.pubkey();

    // true up source epoch if transient
    if move_source_type == StakeLifecycle::Activating
        || move_source_type == StakeLifecycle::Deactivating
    {
        let clock = context.banks_client.get_sysvar::<Clock>().await.unwrap();
        if let StakeStateV2::Stake(_, ref mut stake, _) = &mut source_stake_state {
            match move_source_type {
                StakeLifecycle::Activating => stake.delegation.activation_epoch = clock.epoch,
                StakeLifecycle::Deactivating => stake.delegation.deactivation_epoch = clock.epoch,
                _ => (),
            }
        }

        source_account.data = bincode::serialize(&source_stake_state).unwrap();
        context.set_account(&move_source, &source_account.into());
    }

    // if we activated the initial amount we need to top up with the test lamports
    if move_source_type == StakeLifecycle::Active {
        transfer(&mut context, &move_source, source_excess).await;
    }
    if move_dest_type == StakeLifecycle::Active {
        transfer(&mut context, &move_dest, dest_excess).await;
    }

    // clear out state failures
    if move_source_type == StakeLifecycle::Activating
        || move_source_type == StakeLifecycle::Deactivating
        || move_dest_type == StakeLifecycle::Deactivating
    {
        let instruction = ixn::move_lamports(
            &move_source,
            &move_dest,
            &staker_keypair.pubkey(),
            source_excess,
        );

        process_instruction(&mut context, &instruction, &vec![&staker_keypair])
            .await
            .unwrap_err();
        return;
    }

    // overshoot and fail for underfunded source
    let instruction = ixn::move_lamports(
        &move_source,
        &move_dest,
        &staker_keypair.pubkey(),
        source_excess + 1,
    );

    let e = process_instruction(&mut context, &instruction, &vec![&staker_keypair])
        .await
        .unwrap_err();
    assert_eq!(e, ProgramError::InvalidArgument);

    let (_, _, before_source_lamports) =
        get_stake_account(&mut context.banks_client, &move_source).await;
    let (_, _, before_dest_lamports) =
        get_stake_account(&mut context.banks_client, &move_dest).await;

    // now properly move the full excess
    let instruction = ixn::move_lamports(
        &move_source,
        &move_dest,
        &staker_keypair.pubkey(),
        source_excess,
    );

    test_instruction_with_missing_signers(&mut context, &instruction, &vec![&staker_keypair]).await;

    let (_, _, after_source_lamports) =
        get_stake_account(&mut context.banks_client, &move_source).await;
    let source_effective_stake = get_effective_stake(&mut context.banks_client, &move_source).await;

    // source activation didnt change
    assert_eq!(source_effective_stake, source_staked_amount);

    // source lamports are right
    assert_eq!(
        after_source_lamports,
        before_source_lamports - minimum_delegation
    );
    assert_eq!(
        after_source_lamports,
        source_effective_stake + rent_exempt_reserve
    );

    let (_, _, after_dest_lamports) =
        get_stake_account(&mut context.banks_client, &move_dest).await;
    let dest_effective_stake = get_effective_stake(&mut context.banks_client, &move_dest).await;

    // dest activation didnt change
    assert_eq!(dest_effective_stake, dest_staked_amount);

    // dest lamports are right
    assert_eq!(
        after_dest_lamports,
        before_dest_lamports + minimum_delegation
    );
    assert_eq!(
        after_dest_lamports,
        dest_effective_stake + rent_exempt_reserve + source_excess + dest_excess
    );
}

#[test_matrix(
    [(StakeLifecycle::Active, StakeLifecycle::Uninitialized),
     (StakeLifecycle::Uninitialized, StakeLifecycle::Initialized),
     (StakeLifecycle::Uninitialized, StakeLifecycle::Uninitialized)],
    [false, true]
)]
#[tokio::test]
async fn test_move_uninitialized_fail(
    move_types: (StakeLifecycle, StakeLifecycle),
    move_lamports: bool,
) {
    let mut context = program_test().start_with_context().await;
    let accounts = Accounts::default();
    accounts.initialize(&mut context).await;

    let minimum_delegation = get_minimum_delegation(&mut context).await;
    let source_staked_amount = minimum_delegation * 2;

    let (move_source_type, move_dest_type) = move_types;

    let (move_source_keypair, staker_keypair, withdrawer_keypair) = move_source_type
        .new_stake_account(
            &mut context,
            &accounts.vote_account.pubkey(),
            source_staked_amount,
        )
        .await;
    let move_source = move_source_keypair.pubkey();

    let move_dest_keypair = Keypair::new();
    move_dest_type
        .new_stake_account_fully_specified(
            &mut context,
            &accounts.vote_account.pubkey(),
            0,
            &move_dest_keypair,
            &staker_keypair,
            &withdrawer_keypair,
            &Lockup::default(),
        )
        .await;
    let move_dest = move_dest_keypair.pubkey();

    let source_signer = if move_source_type == StakeLifecycle::Uninitialized {
        &move_source_keypair
    } else {
        &staker_keypair
    };

    let instruction = if move_lamports {
        ixn::move_lamports(
            &move_source,
            &move_dest,
            &source_signer.pubkey(),
            minimum_delegation,
        )
    } else {
        ixn::move_stake(
            &move_source,
            &move_dest,
            &source_signer.pubkey(),
            minimum_delegation,
        )
    };

    let e = process_instruction(&mut context, &instruction, &vec![source_signer])
        .await
        .unwrap_err();
    assert_eq!(e, ProgramError::InvalidAccountData);
}

#[test_matrix(
    [StakeLifecycle::Initialized, StakeLifecycle::Active, StakeLifecycle::Deactive],
    [StakeLifecycle::Initialized, StakeLifecycle::Activating, StakeLifecycle::Active, StakeLifecycle::Deactive],
    [false, true]
)]
#[tokio::test]
async fn test_move_general_fail(
    move_source_type: StakeLifecycle,
    move_dest_type: StakeLifecycle,
    move_lamports: bool,
) {
    // the test_matrix includes all valid source/dest combinations for MoveLamports
    // we dont test invalid combinations because they would fail regardless of the fail cases we test here
    // valid source/dest for MoveStake are a strict subset of MoveLamports
    // source must be active, and dest must be active or inactive. so we skip the additional invalid MoveStake cases
    if !move_lamports
        && (move_source_type != StakeLifecycle::Active
            || move_dest_type == StakeLifecycle::Activating)
    {
        return;
    }

    let mut context = program_test().start_with_context().await;
    let accounts = Accounts::default();
    accounts.initialize(&mut context).await;

    let minimum_delegation = get_minimum_delegation(&mut context).await;
    let source_staked_amount = minimum_delegation * 2;

    let in_force_lockup = {
        let clock = context.banks_client.get_sysvar::<Clock>().await.unwrap();
        Lockup {
            unix_timestamp: 0,
            epoch: clock.epoch + 1_000_000,
            custodian: Pubkey::new_unique(),
        }
    };

    let mk_ixn = if move_lamports {
        ixn::move_lamports
    } else {
        ixn::move_stake
    };

    // we can reuse source but will need a lot of dest
    let (move_source_keypair, staker_keypair, withdrawer_keypair) = move_source_type
        .new_stake_account(
            &mut context,
            &accounts.vote_account.pubkey(),
            source_staked_amount,
        )
        .await;
    let move_source = move_source_keypair.pubkey();
    transfer(&mut context, &move_source, minimum_delegation).await;

    // self-move fails
    // NOTE this error type is an artifact of the native program interface
    // when we move to bpf, it should actually hit the processor error
    let instruction = mk_ixn(
        &move_source,
        &move_source,
        &staker_keypair.pubkey(),
        minimum_delegation,
    );
    let e = process_instruction(&mut context, &instruction, &vec![&staker_keypair])
        .await
        .unwrap_err();
    assert_eq!(e, ProgramError::AccountBorrowFailed);

    // first we make a "normal" move dest
    {
        let move_dest_keypair = Keypair::new();
        move_dest_type
            .new_stake_account_fully_specified(
                &mut context,
                &accounts.vote_account.pubkey(),
                minimum_delegation,
                &move_dest_keypair,
                &staker_keypair,
                &withdrawer_keypair,
                &Lockup::default(),
            )
            .await;
        let move_dest = move_dest_keypair.pubkey();

        // zero move fails
        let instruction = mk_ixn(&move_source, &move_dest, &staker_keypair.pubkey(), 0);
        let e = process_instruction(&mut context, &instruction, &vec![&staker_keypair])
            .await
            .unwrap_err();
        assert_eq!(e, ProgramError::InvalidArgument);

        // sign with withdrawer fails
        let instruction = mk_ixn(
            &move_source,
            &move_dest,
            &withdrawer_keypair.pubkey(),
            minimum_delegation,
        );
        let e = process_instruction(&mut context, &instruction, &vec![&withdrawer_keypair])
            .await
            .unwrap_err();
        assert_eq!(e, ProgramError::MissingRequiredSignature);

        // good place to test source lockup
        let move_locked_source_keypair = Keypair::new();
        move_source_type
            .new_stake_account_fully_specified(
                &mut context,
                &accounts.vote_account.pubkey(),
                source_staked_amount,
                &move_locked_source_keypair,
                &staker_keypair,
                &withdrawer_keypair,
                &in_force_lockup,
            )
            .await;
        let move_locked_source = move_locked_source_keypair.pubkey();
        transfer(&mut context, &move_locked_source, minimum_delegation).await;

        let instruction = mk_ixn(
            &move_locked_source,
            &move_dest,
            &staker_keypair.pubkey(),
            minimum_delegation,
        );
        let e = process_instruction(&mut context, &instruction, &vec![&staker_keypair])
            .await
            .unwrap_err();
        assert_eq!(e, StakeError::MergeMismatch.into());
    }

    // staker mismatch
    {
        let move_dest_keypair = Keypair::new();
        let throwaway = Keypair::new();
        move_dest_type
            .new_stake_account_fully_specified(
                &mut context,
                &accounts.vote_account.pubkey(),
                minimum_delegation,
                &move_dest_keypair,
                &throwaway,
                &withdrawer_keypair,
                &Lockup::default(),
            )
            .await;
        let move_dest = move_dest_keypair.pubkey();

        let instruction = mk_ixn(
            &move_source,
            &move_dest,
            &staker_keypair.pubkey(),
            minimum_delegation,
        );
        let e = process_instruction(&mut context, &instruction, &vec![&staker_keypair])
            .await
            .unwrap_err();
        assert_eq!(e, StakeError::MergeMismatch.into());

        let instruction = mk_ixn(
            &move_source,
            &move_dest,
            &throwaway.pubkey(),
            minimum_delegation,
        );
        let e = process_instruction(&mut context, &instruction, &vec![&throwaway])
            .await
            .unwrap_err();
        assert_eq!(e, ProgramError::MissingRequiredSignature);
    }

    // withdrawer mismatch
    {
        let move_dest_keypair = Keypair::new();
        let throwaway = Keypair::new();
        move_dest_type
            .new_stake_account_fully_specified(
                &mut context,
                &accounts.vote_account.pubkey(),
                minimum_delegation,
                &move_dest_keypair,
                &staker_keypair,
                &throwaway,
                &Lockup::default(),
            )
            .await;
        let move_dest = move_dest_keypair.pubkey();

        let instruction = mk_ixn(
            &move_source,
            &move_dest,
            &staker_keypair.pubkey(),
            minimum_delegation,
        );
        let e = process_instruction(&mut context, &instruction, &vec![&staker_keypair])
            .await
            .unwrap_err();
        assert_eq!(e, StakeError::MergeMismatch.into());

        let instruction = mk_ixn(
            &move_source,
            &move_dest,
            &throwaway.pubkey(),
            minimum_delegation,
        );
        let e = process_instruction(&mut context, &instruction, &vec![&throwaway])
            .await
            .unwrap_err();
        assert_eq!(e, ProgramError::MissingRequiredSignature);
    }

    // dest lockup
    {
        let move_dest_keypair = Keypair::new();
        move_dest_type
            .new_stake_account_fully_specified(
                &mut context,
                &accounts.vote_account.pubkey(),
                minimum_delegation,
                &move_dest_keypair,
                &staker_keypair,
                &withdrawer_keypair,
                &in_force_lockup,
            )
            .await;
        let move_dest = move_dest_keypair.pubkey();

        let instruction = mk_ixn(
            &move_source,
            &move_dest,
            &staker_keypair.pubkey(),
            minimum_delegation,
        );
        let e = process_instruction(&mut context, &instruction, &vec![&staker_keypair])
            .await
            .unwrap_err();
        assert_eq!(e, StakeError::MergeMismatch.into());
    }

    // lastly we test different vote accounts for move_stake
    if !move_lamports && move_dest_type == StakeLifecycle::Active {
        let dest_vote_account_keypair = Keypair::new();
        create_vote(
            &mut context,
            &Keypair::new(),
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            &dest_vote_account_keypair,
        )
        .await;

        let move_dest_keypair = Keypair::new();
        move_dest_type
            .new_stake_account_fully_specified(
                &mut context,
                &dest_vote_account_keypair.pubkey(),
                minimum_delegation,
                &move_dest_keypair,
                &staker_keypair,
                &withdrawer_keypair,
                &Lockup::default(),
            )
            .await;
        let move_dest = move_dest_keypair.pubkey();

        let instruction = mk_ixn(
            &move_source,
            &move_dest,
            &staker_keypair.pubkey(),
            minimum_delegation,
        );
        let e = process_instruction(&mut context, &instruction, &vec![&staker_keypair])
            .await
            .unwrap_err();
        assert_eq!(e, StakeError::VoteAddressMismatch.into());
    }
}

// this test is only to be sure the feature gate is safe
// once the feature has been activated, this can all be deleted
#[test_matrix(
    [StakeLifecycle::Initialized, StakeLifecycle::Active, StakeLifecycle::Deactive],
    [StakeLifecycle::Initialized, StakeLifecycle::Activating, StakeLifecycle::Active, StakeLifecycle::Deactive],
    [false, true]
)]
#[tokio::test]
async fn test_move_feature_gate_fail(
    move_source_type: StakeLifecycle,
    move_dest_type: StakeLifecycle,
    move_lamports: bool,
) {
    // the test_matrix includes all valid source/dest combinations for MoveLamports
    // we dont test invalid combinations because they would fail regardless of the fail cases we test here
    // valid source/dest for MoveStake are a strict subset of MoveLamports
    // source must be active, and dest must be active or inactive. so we skip the additional invalid MoveStake cases
    if !move_lamports
        && (move_source_type != StakeLifecycle::Active
            || move_dest_type == StakeLifecycle::Activating)
    {
        return;
    }

    let mut context = program_test_without_features(&[move_stake_and_move_lamports_ixs::id()])
        .start_with_context()
        .await;

    let accounts = Accounts::default();
    accounts.initialize(&mut context).await;

    let minimum_delegation = get_minimum_delegation(&mut context).await;
    let source_staked_amount = minimum_delegation * 2;

    let mk_ixn = if move_lamports {
        ixn::move_lamports
    } else {
        ixn::move_stake
    };

    let (move_source_keypair, staker_keypair, withdrawer_keypair) = move_source_type
        .new_stake_account(
            &mut context,
            &accounts.vote_account.pubkey(),
            source_staked_amount,
        )
        .await;
    let move_source = move_source_keypair.pubkey();
    transfer(&mut context, &move_source, minimum_delegation).await;

    let move_dest_keypair = Keypair::new();
    move_dest_type
        .new_stake_account_fully_specified(
            &mut context,
            &accounts.vote_account.pubkey(),
            minimum_delegation,
            &move_dest_keypair,
            &staker_keypair,
            &withdrawer_keypair,
            &Lockup::default(),
        )
        .await;
    let move_dest = move_dest_keypair.pubkey();

    let instruction = mk_ixn(
        &move_source,
        &move_dest,
        &staker_keypair.pubkey(),
        minimum_delegation,
    );
    let e = process_instruction(&mut context, &instruction, &vec![&staker_keypair])
        .await
        .unwrap_err();
    assert_eq!(e, ProgramError::InvalidInstructionData);
}
