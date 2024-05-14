//! Stake state
//! * delegate stakes to vote accounts
//! * keep track of rewards
//! * own mining pools

#[deprecated(
    since = "1.8.0",
    note = "Please use `solana_sdk::stake::state` or `solana_program::stake::state` instead"
)]
pub use solana_sdk::stake::state::*;
use {
    solana_program_runtime::{ic_msg, invoke_context::InvokeContext},
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount},
        account_utils::StateMut,
        clock::{Clock, Epoch},
        feature_set::{self, FeatureSet},
        instruction::{checked_add, InstructionError},
        pubkey::Pubkey,
        rent::Rent,
        stake::{
            instruction::{LockupArgs, StakeError},
            program::id,
            stake_flags::StakeFlags,
            tools::{acceptable_reference_epoch_credits, eligible_for_deactivate_delinquent},
        },
        stake_history::{StakeHistory, StakeHistoryEntry},
        transaction_context::{
            BorrowedAccount, IndexOfAccount, InstructionContext, TransactionContext,
        },
    },
    solana_vote_program::vote_state::{self, VoteState, VoteStateVersions},
    std::{collections::HashSet, convert::TryFrom},
};

// utility function, used by Stakes, tests
pub fn from<T: ReadableAccount + StateMut<StakeStateV2>>(account: &T) -> Option<StakeStateV2> {
    account.state().ok()
}

pub fn stake_from<T: ReadableAccount + StateMut<StakeStateV2>>(account: &T) -> Option<Stake> {
    from(account).and_then(|state: StakeStateV2| state.stake())
}

pub fn delegation_from(account: &AccountSharedData) -> Option<Delegation> {
    from(account).and_then(|state: StakeStateV2| state.delegation())
}

pub fn authorized_from(account: &AccountSharedData) -> Option<Authorized> {
    from(account).and_then(|state: StakeStateV2| state.authorized())
}

pub fn lockup_from<T: ReadableAccount + StateMut<StakeStateV2>>(account: &T) -> Option<Lockup> {
    from(account).and_then(|state: StakeStateV2| state.lockup())
}

pub fn meta_from(account: &AccountSharedData) -> Option<Meta> {
    from(account).and_then(|state: StakeStateV2| state.meta())
}

pub(crate) fn new_warmup_cooldown_rate_epoch(invoke_context: &InvokeContext) -> Option<Epoch> {
    let epoch_schedule = invoke_context
        .get_sysvar_cache()
        .get_epoch_schedule()
        .unwrap();
    invoke_context
        .feature_set
        .new_warmup_cooldown_rate_epoch(epoch_schedule.as_ref())
}

fn get_stake_status(
    invoke_context: &InvokeContext,
    stake: &Stake,
    clock: &Clock,
) -> Result<StakeActivationStatus, InstructionError> {
    let stake_history = invoke_context.get_sysvar_cache().get_stake_history()?;
    Ok(stake.delegation.stake_activating_and_deactivating(
        clock.epoch,
        &stake_history,
        new_warmup_cooldown_rate_epoch(invoke_context),
    ))
}

fn redelegate_stake(
    invoke_context: &InvokeContext,
    stake: &mut Stake,
    stake_lamports: u64,
    voter_pubkey: &Pubkey,
    vote_state: &VoteState,
    clock: &Clock,
    stake_history: &StakeHistory,
) -> Result<(), StakeError> {
    let new_rate_activation_epoch = new_warmup_cooldown_rate_epoch(invoke_context);
    // If stake is currently active:
    if stake.stake(clock.epoch, stake_history, new_rate_activation_epoch) != 0 {
        let stake_lamports_ok = if invoke_context
            .feature_set
            .is_active(&feature_set::stake_redelegate_instruction::id())
        {
            // When a stake account is redelegated, the delegated lamports from the source stake
            // account are transferred to a new stake account. Do not permit the deactivation of
            // the source stake account to be rescinded, by more generally requiring the delegation
            // be configured with the expected amount of stake lamports before rescinding.
            stake_lamports >= stake.delegation.stake
        } else {
            true
        };

        // If pubkey of new voter is the same as current,
        // and we are scheduled to start deactivating this epoch,
        // we rescind deactivation
        if stake.delegation.voter_pubkey == *voter_pubkey
            && clock.epoch == stake.delegation.deactivation_epoch
            && stake_lamports_ok
        {
            stake.delegation.deactivation_epoch = std::u64::MAX;
            return Ok(());
        } else {
            // can't redelegate to another pubkey if stake is active.
            return Err(StakeError::TooSoonToRedelegate);
        }
    }
    // Either the stake is freshly activated, is active but has been
    // deactivated this epoch, or has fully de-activated.
    // Redelegation implies either re-activation or un-deactivation

    stake.delegation.stake = stake_lamports;
    stake.delegation.activation_epoch = clock.epoch;
    stake.delegation.deactivation_epoch = std::u64::MAX;
    stake.delegation.voter_pubkey = *voter_pubkey;
    stake.credits_observed = vote_state.credits();
    Ok(())
}

pub(crate) fn new_stake(
    stake: u64,
    voter_pubkey: &Pubkey,
    vote_state: &VoteState,
    activation_epoch: Epoch,
) -> Stake {
    Stake {
        delegation: Delegation::new(voter_pubkey, stake, activation_epoch),
        credits_observed: vote_state.credits(),
    }
}

pub fn initialize(
    stake_account: &mut BorrowedAccount,
    authorized: &Authorized,
    lockup: &Lockup,
    rent: &Rent,
) -> Result<(), InstructionError> {
    if stake_account.get_data().len() != StakeStateV2::size_of() {
        return Err(InstructionError::InvalidAccountData);
    }

    if let StakeStateV2::Uninitialized = stake_account.get_state()? {
        let rent_exempt_reserve = rent.minimum_balance(stake_account.get_data().len());
        if stake_account.get_lamports() >= rent_exempt_reserve {
            stake_account.set_state(&StakeStateV2::Initialized(Meta {
                rent_exempt_reserve,
                authorized: *authorized,
                lockup: *lockup,
            }))
        } else {
            Err(InstructionError::InsufficientFunds)
        }
    } else {
        Err(InstructionError::InvalidAccountData)
    }
}

/// Authorize the given pubkey to manage stake (deactivate, withdraw). This may be called
/// multiple times, but will implicitly withdraw authorization from the previously authorized
/// staker. The default staker is the owner of the stake account's pubkey.
pub fn authorize(
    stake_account: &mut BorrowedAccount,
    signers: &HashSet<Pubkey>,
    new_authority: &Pubkey,
    stake_authorize: StakeAuthorize,
    clock: &Clock,
    custodian: Option<&Pubkey>,
) -> Result<(), InstructionError> {
    match stake_account.get_state()? {
        StakeStateV2::Stake(mut meta, stake, stake_flags) => {
            meta.authorized.authorize(
                signers,
                new_authority,
                stake_authorize,
                Some((&meta.lockup, clock, custodian)),
            )?;
            stake_account.set_state(&StakeStateV2::Stake(meta, stake, stake_flags))
        }
        StakeStateV2::Initialized(mut meta) => {
            meta.authorized.authorize(
                signers,
                new_authority,
                stake_authorize,
                Some((&meta.lockup, clock, custodian)),
            )?;
            stake_account.set_state(&StakeStateV2::Initialized(meta))
        }
        _ => Err(InstructionError::InvalidAccountData),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn authorize_with_seed(
    transaction_context: &TransactionContext,
    instruction_context: &InstructionContext,
    stake_account: &mut BorrowedAccount,
    authority_base_index: IndexOfAccount,
    authority_seed: &str,
    authority_owner: &Pubkey,
    new_authority: &Pubkey,
    stake_authorize: StakeAuthorize,
    clock: &Clock,
    custodian: Option<&Pubkey>,
) -> Result<(), InstructionError> {
    let mut signers = HashSet::default();
    if instruction_context.is_instruction_account_signer(authority_base_index)? {
        let base_pubkey = transaction_context.get_key_of_account_at_index(
            instruction_context
                .get_index_of_instruction_account_in_transaction(authority_base_index)?,
        )?;
        signers.insert(Pubkey::create_with_seed(
            base_pubkey,
            authority_seed,
            authority_owner,
        )?);
    }
    authorize(
        stake_account,
        &signers,
        new_authority,
        stake_authorize,
        clock,
        custodian,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn delegate(
    invoke_context: &InvokeContext,
    transaction_context: &TransactionContext,
    instruction_context: &InstructionContext,
    stake_account_index: IndexOfAccount,
    vote_account_index: IndexOfAccount,
    clock: &Clock,
    stake_history: &StakeHistory,
    signers: &HashSet<Pubkey>,
    feature_set: &FeatureSet,
) -> Result<(), InstructionError> {
    let vote_account = instruction_context
        .try_borrow_instruction_account(transaction_context, vote_account_index)?;
    if *vote_account.get_owner() != solana_vote_program::id() {
        return Err(InstructionError::IncorrectProgramId);
    }
    let vote_pubkey = *vote_account.get_key();
    let vote_state = vote_account.get_state::<VoteStateVersions>();
    drop(vote_account);

    let mut stake_account = instruction_context
        .try_borrow_instruction_account(transaction_context, stake_account_index)?;
    match stake_account.get_state()? {
        StakeStateV2::Initialized(meta) => {
            meta.authorized.check(signers, StakeAuthorize::Staker)?;
            let ValidatedDelegatedInfo { stake_amount } =
                validate_delegated_amount(&stake_account, &meta, feature_set)?;
            let stake = new_stake(
                stake_amount,
                &vote_pubkey,
                &vote_state?.convert_to_current(),
                clock.epoch,
            );
            stake_account.set_state(&StakeStateV2::Stake(meta, stake, StakeFlags::empty()))
        }
        StakeStateV2::Stake(meta, mut stake, stake_flags) => {
            meta.authorized.check(signers, StakeAuthorize::Staker)?;
            let ValidatedDelegatedInfo { stake_amount } =
                validate_delegated_amount(&stake_account, &meta, feature_set)?;
            redelegate_stake(
                invoke_context,
                &mut stake,
                stake_amount,
                &vote_pubkey,
                &vote_state?.convert_to_current(),
                clock,
                stake_history,
            )?;
            stake_account.set_state(&StakeStateV2::Stake(meta, stake, stake_flags))
        }
        _ => Err(InstructionError::InvalidAccountData),
    }
}

fn deactivate_stake(
    invoke_context: &InvokeContext,
    stake: &mut Stake,
    stake_flags: &mut StakeFlags,
    epoch: Epoch,
) -> Result<(), InstructionError> {
    if invoke_context
        .feature_set
        .is_active(&feature_set::stake_redelegate_instruction::id())
    {
        if stake_flags.contains(StakeFlags::MUST_FULLY_ACTIVATE_BEFORE_DEACTIVATION_IS_PERMITTED) {
            let stake_history = invoke_context.get_sysvar_cache().get_stake_history()?;
            // when MUST_FULLY_ACTIVATE_BEFORE_DEACTIVATION_IS_PERMITTED flag is set on stake_flags,
            // deactivation is only permitted when the stake delegation activating amount is zero.
            let status = stake.delegation.stake_activating_and_deactivating(
                epoch,
                &stake_history,
                new_warmup_cooldown_rate_epoch(invoke_context),
            );
            if status.activating != 0 {
                Err(InstructionError::from(
                    StakeError::RedelegatedStakeMustFullyActivateBeforeDeactivationIsPermitted,
                ))
            } else {
                stake.deactivate(epoch)?;
                // After deactivation, need to clear `MustFullyActivateBeforeDeactivationIsPermitted` flag if any.
                // So that future activation and deactivation are not subject to that restriction.
                stake_flags
                    .remove(StakeFlags::MUST_FULLY_ACTIVATE_BEFORE_DEACTIVATION_IS_PERMITTED);
                Ok(())
            }
        } else {
            stake.deactivate(epoch)?;
            Ok(())
        }
    } else {
        stake.deactivate(epoch)?;
        Ok(())
    }
}

pub fn deactivate(
    invoke_context: &InvokeContext,
    stake_account: &mut BorrowedAccount,
    clock: &Clock,
    signers: &HashSet<Pubkey>,
) -> Result<(), InstructionError> {
    if let StakeStateV2::Stake(meta, mut stake, mut stake_flags) = stake_account.get_state()? {
        meta.authorized.check(signers, StakeAuthorize::Staker)?;
        deactivate_stake(invoke_context, &mut stake, &mut stake_flags, clock.epoch)?;
        stake_account.set_state(&StakeStateV2::Stake(meta, stake, stake_flags))
    } else {
        Err(InstructionError::InvalidAccountData)
    }
}

pub fn set_lockup(
    stake_account: &mut BorrowedAccount,
    lockup: &LockupArgs,
    signers: &HashSet<Pubkey>,
    clock: &Clock,
) -> Result<(), InstructionError> {
    match stake_account.get_state()? {
        StakeStateV2::Initialized(mut meta) => {
            meta.set_lockup(lockup, signers, clock)?;
            stake_account.set_state(&StakeStateV2::Initialized(meta))
        }
        StakeStateV2::Stake(mut meta, stake, stake_flags) => {
            meta.set_lockup(lockup, signers, clock)?;
            stake_account.set_state(&StakeStateV2::Stake(meta, stake, stake_flags))
        }
        _ => Err(InstructionError::InvalidAccountData),
    }
}

pub fn split(
    invoke_context: &InvokeContext,
    transaction_context: &TransactionContext,
    instruction_context: &InstructionContext,
    stake_account_index: IndexOfAccount,
    lamports: u64,
    split_index: IndexOfAccount,
    signers: &HashSet<Pubkey>,
) -> Result<(), InstructionError> {
    let split =
        instruction_context.try_borrow_instruction_account(transaction_context, split_index)?;
    if *split.get_owner() != id() {
        return Err(InstructionError::IncorrectProgramId);
    }
    if split.get_data().len() != StakeStateV2::size_of() {
        return Err(InstructionError::InvalidAccountData);
    }
    if !matches!(split.get_state()?, StakeStateV2::Uninitialized) {
        return Err(InstructionError::InvalidAccountData);
    }
    let split_lamport_balance = split.get_lamports();
    drop(split);
    let stake_account = instruction_context
        .try_borrow_instruction_account(transaction_context, stake_account_index)?;
    if lamports > stake_account.get_lamports() {
        return Err(InstructionError::InsufficientFunds);
    }
    let stake_state = stake_account.get_state()?;
    drop(stake_account);

    match stake_state {
        StakeStateV2::Stake(meta, mut stake, stake_flags) => {
            meta.authorized.check(signers, StakeAuthorize::Staker)?;
            let minimum_delegation = crate::get_minimum_delegation(&invoke_context.feature_set);
            let is_active = {
                let clock = invoke_context.get_sysvar_cache().get_clock()?;
                let status = get_stake_status(invoke_context, &stake, &clock)?;
                status.effective > 0
            };
            let validated_split_info = validate_split_amount(
                invoke_context,
                transaction_context,
                instruction_context,
                stake_account_index,
                split_index,
                lamports,
                &meta,
                minimum_delegation,
                is_active,
            )?;

            // split the stake, subtract rent_exempt_balance unless
            // the destination account already has those lamports
            // in place.
            // this means that the new stake account will have a stake equivalent to
            // lamports minus rent_exempt_reserve if it starts out with a zero balance
            let (remaining_stake_delta, split_stake_amount) =
                if validated_split_info.source_remaining_balance == 0 {
                    // If split amount equals the full source stake (as implied by 0
                    // source_remaining_balance), the new split stake must equal the same
                    // amount, regardless of any current lamport balance in the split account.
                    // Since split accounts retain the state of their source account, this
                    // prevents any magic activation of stake by prefunding the split account.
                    //
                    // The new split stake also needs to ignore any positive delta between the
                    // original rent_exempt_reserve and the split_rent_exempt_reserve, in order
                    // to prevent magic activation of stake by splitting between accounts of
                    // different sizes.
                    let remaining_stake_delta = lamports.saturating_sub(meta.rent_exempt_reserve);
                    (remaining_stake_delta, remaining_stake_delta)
                } else {
                    // Otherwise, the new split stake should reflect the entire split
                    // requested, less any lamports needed to cover the split_rent_exempt_reserve.

                    if stake.delegation.stake.saturating_sub(lamports) < minimum_delegation {
                        return Err(StakeError::InsufficientDelegation.into());
                    }

                    (
                        lamports,
                        lamports.saturating_sub(
                            validated_split_info
                                .destination_rent_exempt_reserve
                                .saturating_sub(split_lamport_balance),
                        ),
                    )
                };

            if split_stake_amount < minimum_delegation {
                return Err(StakeError::InsufficientDelegation.into());
            }

            let split_stake = stake.split(remaining_stake_delta, split_stake_amount)?;
            let mut split_meta = meta;
            split_meta.rent_exempt_reserve = validated_split_info.destination_rent_exempt_reserve;

            let mut stake_account = instruction_context
                .try_borrow_instruction_account(transaction_context, stake_account_index)?;
            stake_account.set_state(&StakeStateV2::Stake(meta, stake, stake_flags))?;
            drop(stake_account);
            let mut split = instruction_context
                .try_borrow_instruction_account(transaction_context, split_index)?;
            split.set_state(&StakeStateV2::Stake(split_meta, split_stake, stake_flags))?;
        }
        StakeStateV2::Initialized(meta) => {
            meta.authorized.check(signers, StakeAuthorize::Staker)?;
            let validated_split_info = validate_split_amount(
                invoke_context,
                transaction_context,
                instruction_context,
                stake_account_index,
                split_index,
                lamports,
                &meta,
                0, // additional_required_lamports
                false,
            )?;
            let mut split_meta = meta;
            split_meta.rent_exempt_reserve = validated_split_info.destination_rent_exempt_reserve;
            let mut split = instruction_context
                .try_borrow_instruction_account(transaction_context, split_index)?;
            split.set_state(&StakeStateV2::Initialized(split_meta))?;
        }
        StakeStateV2::Uninitialized => {
            let stake_pubkey = transaction_context.get_key_of_account_at_index(
                instruction_context
                    .get_index_of_instruction_account_in_transaction(stake_account_index)?,
            )?;
            if !signers.contains(stake_pubkey) {
                return Err(InstructionError::MissingRequiredSignature);
            }
        }
        _ => return Err(InstructionError::InvalidAccountData),
    }

    // Deinitialize state upon zero balance
    let mut stake_account = instruction_context
        .try_borrow_instruction_account(transaction_context, stake_account_index)?;
    if lamports == stake_account.get_lamports() {
        stake_account.set_state(&StakeStateV2::Uninitialized)?;
    }
    drop(stake_account);

    let mut split =
        instruction_context.try_borrow_instruction_account(transaction_context, split_index)?;
    split.checked_add_lamports(lamports)?;
    drop(split);
    let mut stake_account = instruction_context
        .try_borrow_instruction_account(transaction_context, stake_account_index)?;
    stake_account.checked_sub_lamports(lamports)?;
    Ok(())
}

pub fn merge(
    invoke_context: &InvokeContext,
    transaction_context: &TransactionContext,
    instruction_context: &InstructionContext,
    stake_account_index: IndexOfAccount,
    source_account_index: IndexOfAccount,
    clock: &Clock,
    stake_history: &StakeHistory,
    signers: &HashSet<Pubkey>,
) -> Result<(), InstructionError> {
    let mut source_account = instruction_context
        .try_borrow_instruction_account(transaction_context, source_account_index)?;
    // Ensure source isn't spoofed
    if *source_account.get_owner() != id() {
        return Err(InstructionError::IncorrectProgramId);
    }
    // Close the stake_account-reference loophole
    if instruction_context.get_index_of_instruction_account_in_transaction(stake_account_index)?
        == instruction_context
            .get_index_of_instruction_account_in_transaction(source_account_index)?
    {
        return Err(InstructionError::InvalidArgument);
    }
    let mut stake_account = instruction_context
        .try_borrow_instruction_account(transaction_context, stake_account_index)?;

    ic_msg!(invoke_context, "Checking if destination stake is mergeable");
    let stake_merge_kind = MergeKind::get_if_mergeable(
        invoke_context,
        &stake_account.get_state()?,
        stake_account.get_lamports(),
        clock,
        stake_history,
    )?;

    // Authorized staker is allowed to split/merge accounts
    stake_merge_kind
        .meta()
        .authorized
        .check(signers, StakeAuthorize::Staker)?;

    ic_msg!(invoke_context, "Checking if source stake is mergeable");
    let source_merge_kind = MergeKind::get_if_mergeable(
        invoke_context,
        &source_account.get_state()?,
        source_account.get_lamports(),
        clock,
        stake_history,
    )?;

    ic_msg!(invoke_context, "Merging stake accounts");
    if let Some(merged_state) = stake_merge_kind.merge(invoke_context, source_merge_kind, clock)? {
        stake_account.set_state(&merged_state)?;
    }

    // Source is about to be drained, deinitialize its state
    source_account.set_state(&StakeStateV2::Uninitialized)?;

    // Drain the source stake account
    let lamports = source_account.get_lamports();
    source_account.checked_sub_lamports(lamports)?;
    stake_account.checked_add_lamports(lamports)?;
    Ok(())
}

pub fn redelegate(
    invoke_context: &InvokeContext,
    transaction_context: &TransactionContext,
    instruction_context: &InstructionContext,
    stake_account: &mut BorrowedAccount,
    uninitialized_stake_account_index: IndexOfAccount,
    vote_account_index: IndexOfAccount,
    signers: &HashSet<Pubkey>,
) -> Result<(), InstructionError> {
    let clock = invoke_context.get_sysvar_cache().get_clock()?;

    // ensure `uninitialized_stake_account_index` is in the uninitialized state
    let mut uninitialized_stake_account = instruction_context
        .try_borrow_instruction_account(transaction_context, uninitialized_stake_account_index)?;
    if *uninitialized_stake_account.get_owner() != id() {
        ic_msg!(
            invoke_context,
            "expected uninitialized stake account owner to be {}, not {}",
            id(),
            *uninitialized_stake_account.get_owner()
        );
        return Err(InstructionError::IncorrectProgramId);
    }
    if uninitialized_stake_account.get_data().len() != StakeStateV2::size_of() {
        ic_msg!(
            invoke_context,
            "expected uninitialized stake account data len to be {}, not {}",
            StakeStateV2::size_of(),
            uninitialized_stake_account.get_data().len()
        );
        return Err(InstructionError::InvalidAccountData);
    }
    if !matches!(
        uninitialized_stake_account.get_state()?,
        StakeStateV2::Uninitialized
    ) {
        ic_msg!(
            invoke_context,
            "expected uninitialized stake account to be uninitialized",
        );
        return Err(InstructionError::AccountAlreadyInitialized);
    }

    // validate the provided vote account
    let vote_account = instruction_context
        .try_borrow_instruction_account(transaction_context, vote_account_index)?;
    if *vote_account.get_owner() != solana_vote_program::id() {
        ic_msg!(
            invoke_context,
            "expected vote account owner to be {}, not {}",
            solana_vote_program::id(),
            *vote_account.get_owner()
        );
        return Err(InstructionError::IncorrectProgramId);
    }
    let vote_pubkey = *vote_account.get_key();
    let vote_state = vote_account.get_state::<VoteStateVersions>()?;

    let (stake_meta, effective_stake) =
        if let StakeStateV2::Stake(meta, stake, _stake_flags) = stake_account.get_state()? {
            let status = get_stake_status(invoke_context, &stake, &clock)?;
            if status.effective == 0 || status.activating != 0 || status.deactivating != 0 {
                ic_msg!(invoke_context, "stake is not active");
                return Err(StakeError::RedelegateTransientOrInactiveStake.into());
            }

            // Deny redelegating to the same vote account. This is nonsensical and could be used to
            // grief the global stake warm-up/cool-down rate
            if stake.delegation.voter_pubkey == vote_pubkey {
                ic_msg!(
                    invoke_context,
                    "redelegating to the same vote account not permitted"
                );
                return Err(StakeError::RedelegateToSameVoteAccount.into());
            }

            (meta, status.effective)
        } else {
            ic_msg!(invoke_context, "invalid stake account data",);
            return Err(InstructionError::InvalidAccountData);
        };

    // deactivate `stake_account`
    //
    // Note: This function also ensures `signers` contains the `StakeAuthorize::Staker`
    deactivate(invoke_context, stake_account, &clock, signers)?;

    // transfer the effective stake to the uninitialized stake account
    stake_account.checked_sub_lamports(effective_stake)?;
    uninitialized_stake_account.checked_add_lamports(effective_stake)?;

    // initialize and schedule `uninitialized_stake_account` for activation
    let sysvar_cache = invoke_context.get_sysvar_cache();
    let rent = sysvar_cache.get_rent()?;
    let mut uninitialized_stake_meta = stake_meta;
    uninitialized_stake_meta.rent_exempt_reserve =
        rent.minimum_balance(uninitialized_stake_account.get_data().len());

    let ValidatedDelegatedInfo { stake_amount } = validate_delegated_amount(
        &uninitialized_stake_account,
        &uninitialized_stake_meta,
        &invoke_context.feature_set,
    )?;
    uninitialized_stake_account.set_state(&StakeStateV2::Stake(
        uninitialized_stake_meta,
        new_stake(
            stake_amount,
            &vote_pubkey,
            &vote_state.convert_to_current(),
            clock.epoch,
        ),
        StakeFlags::MUST_FULLY_ACTIVATE_BEFORE_DEACTIVATION_IS_PERMITTED,
    ))?;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn withdraw(
    transaction_context: &TransactionContext,
    instruction_context: &InstructionContext,
    stake_account_index: IndexOfAccount,
    lamports: u64,
    to_index: IndexOfAccount,
    clock: &Clock,
    stake_history: &StakeHistory,
    withdraw_authority_index: IndexOfAccount,
    custodian_index: Option<IndexOfAccount>,
    new_rate_activation_epoch: Option<Epoch>,
) -> Result<(), InstructionError> {
    let withdraw_authority_pubkey = transaction_context.get_key_of_account_at_index(
        instruction_context
            .get_index_of_instruction_account_in_transaction(withdraw_authority_index)?,
    )?;
    if !instruction_context.is_instruction_account_signer(withdraw_authority_index)? {
        return Err(InstructionError::MissingRequiredSignature);
    }
    let mut signers = HashSet::new();
    signers.insert(*withdraw_authority_pubkey);

    let mut stake_account = instruction_context
        .try_borrow_instruction_account(transaction_context, stake_account_index)?;
    let (lockup, reserve, is_staked) = match stake_account.get_state()? {
        StakeStateV2::Stake(meta, stake, _stake_flag) => {
            meta.authorized
                .check(&signers, StakeAuthorize::Withdrawer)?;
            // if we have a deactivation epoch and we're in cooldown
            let staked = if clock.epoch >= stake.delegation.deactivation_epoch {
                stake
                    .delegation
                    .stake(clock.epoch, stake_history, new_rate_activation_epoch)
            } else {
                // Assume full stake if the stake account hasn't been
                //  de-activated, because in the future the exposed stake
                //  might be higher than stake.stake() due to warmup
                stake.delegation.stake
            };

            let staked_and_reserve = checked_add(staked, meta.rent_exempt_reserve)?;
            (meta.lockup, staked_and_reserve, staked != 0)
        }
        StakeStateV2::Initialized(meta) => {
            meta.authorized
                .check(&signers, StakeAuthorize::Withdrawer)?;
            // stake accounts must have a balance >= rent_exempt_reserve
            (meta.lockup, meta.rent_exempt_reserve, false)
        }
        StakeStateV2::Uninitialized => {
            if !signers.contains(stake_account.get_key()) {
                return Err(InstructionError::MissingRequiredSignature);
            }
            (Lockup::default(), 0, false) // no lockup, no restrictions
        }
        _ => return Err(InstructionError::InvalidAccountData),
    };

    // verify that lockup has expired or that the withdrawal is signed by
    //   the custodian, both epoch and unix_timestamp must have passed
    let custodian_pubkey = if let Some(custodian_index) = custodian_index {
        if instruction_context.is_instruction_account_signer(custodian_index)? {
            Some(
                transaction_context.get_key_of_account_at_index(
                    instruction_context
                        .get_index_of_instruction_account_in_transaction(custodian_index)?,
                )?,
            )
        } else {
            None
        }
    } else {
        None
    };
    if lockup.is_in_force(clock, custodian_pubkey) {
        return Err(StakeError::LockupInForce.into());
    }

    let lamports_and_reserve = checked_add(lamports, reserve)?;
    // if the stake is active, we mustn't allow the account to go away
    if is_staked // line coverage for branch coverage
            && lamports_and_reserve > stake_account.get_lamports()
    {
        return Err(InstructionError::InsufficientFunds);
    }

    if lamports != stake_account.get_lamports() // not a full withdrawal
            && lamports_and_reserve > stake_account.get_lamports()
    {
        assert!(!is_staked);
        return Err(InstructionError::InsufficientFunds);
    }

    // Deinitialize state upon zero balance
    if lamports == stake_account.get_lamports() {
        stake_account.set_state(&StakeStateV2::Uninitialized)?;
    }

    stake_account.checked_sub_lamports(lamports)?;
    drop(stake_account);
    let mut to =
        instruction_context.try_borrow_instruction_account(transaction_context, to_index)?;
    to.checked_add_lamports(lamports)?;
    Ok(())
}

pub(crate) fn deactivate_delinquent(
    invoke_context: &InvokeContext,
    transaction_context: &TransactionContext,
    instruction_context: &InstructionContext,
    stake_account: &mut BorrowedAccount,
    delinquent_vote_account_index: IndexOfAccount,
    reference_vote_account_index: IndexOfAccount,
    current_epoch: Epoch,
) -> Result<(), InstructionError> {
    let delinquent_vote_account_pubkey = transaction_context.get_key_of_account_at_index(
        instruction_context
            .get_index_of_instruction_account_in_transaction(delinquent_vote_account_index)?,
    )?;
    let delinquent_vote_account = instruction_context
        .try_borrow_instruction_account(transaction_context, delinquent_vote_account_index)?;
    if *delinquent_vote_account.get_owner() != solana_vote_program::id() {
        return Err(InstructionError::IncorrectProgramId);
    }
    let delinquent_vote_state = delinquent_vote_account
        .get_state::<VoteStateVersions>()?
        .convert_to_current();

    let reference_vote_account = instruction_context
        .try_borrow_instruction_account(transaction_context, reference_vote_account_index)?;
    if *reference_vote_account.get_owner() != solana_vote_program::id() {
        return Err(InstructionError::IncorrectProgramId);
    }
    let reference_vote_state = reference_vote_account
        .get_state::<VoteStateVersions>()?
        .convert_to_current();

    if !acceptable_reference_epoch_credits(&reference_vote_state.epoch_credits, current_epoch) {
        return Err(StakeError::InsufficientReferenceVotes.into());
    }

    if let StakeStateV2::Stake(meta, mut stake, mut stake_flags) = stake_account.get_state()? {
        if stake.delegation.voter_pubkey != *delinquent_vote_account_pubkey {
            return Err(StakeError::VoteAddressMismatch.into());
        }

        // Deactivate the stake account if its delegated vote account has never voted or has not
        // voted in the last `MINIMUM_DELINQUENT_EPOCHS_FOR_DEACTIVATION`
        if eligible_for_deactivate_delinquent(&delinquent_vote_state.epoch_credits, current_epoch) {
            deactivate_stake(invoke_context, &mut stake, &mut stake_flags, current_epoch)?;
            stake_account.set_state(&StakeStateV2::Stake(meta, stake, stake_flags))
        } else {
            Err(StakeError::MinimumDelinquentEpochsForDeactivationNotMet.into())
        }
    } else {
        Err(InstructionError::InvalidAccountData)
    }
}

/// After calling `validate_delegated_amount()`, this struct contains calculated values that are used
/// by the caller.
struct ValidatedDelegatedInfo {
    stake_amount: u64,
}

/// Ensure the stake delegation amount is valid.  This checks that the account meets the minimum
/// balance requirements of delegated stake.  If not, return an error.
fn validate_delegated_amount(
    account: &BorrowedAccount,
    meta: &Meta,
    feature_set: &FeatureSet,
) -> Result<ValidatedDelegatedInfo, InstructionError> {
    let stake_amount = account
        .get_lamports()
        .saturating_sub(meta.rent_exempt_reserve); // can't stake the rent

    // Stake accounts may be initialized with a stake amount below the minimum delegation so check
    // that the minimum is met before delegation.
    if stake_amount < crate::get_minimum_delegation(feature_set) {
        return Err(StakeError::InsufficientDelegation.into());
    }
    Ok(ValidatedDelegatedInfo { stake_amount })
}

/// After calling `validate_split_amount()`, this struct contains calculated values that are used
/// by the caller.
#[derive(Copy, Clone, Debug, Default)]
struct ValidatedSplitInfo {
    source_remaining_balance: u64,
    destination_rent_exempt_reserve: u64,
}

/// Ensure the split amount is valid.  This checks the source and destination accounts meet the
/// minimum balance requirements, which is the rent exempt reserve plus the minimum stake
/// delegation, and that the source account has enough lamports for the request split amount.  If
/// not, return an error.
fn validate_split_amount(
    invoke_context: &InvokeContext,
    transaction_context: &TransactionContext,
    instruction_context: &InstructionContext,
    source_account_index: IndexOfAccount,
    destination_account_index: IndexOfAccount,
    lamports: u64,
    source_meta: &Meta,
    additional_required_lamports: u64,
    source_is_active: bool,
) -> Result<ValidatedSplitInfo, InstructionError> {
    let source_account = instruction_context
        .try_borrow_instruction_account(transaction_context, source_account_index)?;
    let source_lamports = source_account.get_lamports();
    drop(source_account);
    let destination_account = instruction_context
        .try_borrow_instruction_account(transaction_context, destination_account_index)?;
    let destination_lamports = destination_account.get_lamports();
    let destination_data_len = destination_account.get_data().len();
    drop(destination_account);

    // Split amount has to be something
    if lamports == 0 {
        return Err(InstructionError::InsufficientFunds);
    }

    // Obviously cannot split more than what the source account has
    if lamports > source_lamports {
        return Err(InstructionError::InsufficientFunds);
    }

    // Verify that the source account still has enough lamports left after splitting:
    // EITHER at least the minimum balance, OR zero (in this case the source
    // account is transferring all lamports to new destination account, and the source
    // account will be closed)
    let source_minimum_balance = source_meta
        .rent_exempt_reserve
        .saturating_add(additional_required_lamports);
    let source_remaining_balance = source_lamports.saturating_sub(lamports);
    if source_remaining_balance == 0 {
        // full amount is a withdrawal
        // nothing to do here
    } else if source_remaining_balance < source_minimum_balance {
        // the remaining balance is too low to do the split
        return Err(InstructionError::InsufficientFunds);
    } else {
        // all clear!
        // nothing to do here
    }

    let rent = invoke_context.get_sysvar_cache().get_rent()?;
    let destination_rent_exempt_reserve = rent.minimum_balance(destination_data_len);

    // If the source is active stake, one of these criteria must be met:
    // 1. the destination account must be prefunded with at least the rent-exempt reserve, or
    // 2. the split must consume 100% of the source
    if source_is_active
        && source_remaining_balance != 0
        && destination_lamports < destination_rent_exempt_reserve
    {
        return Err(InstructionError::InsufficientFunds);
    }

    // Verify the destination account meets the minimum balance requirements
    // This must handle:
    // 1. The destination account having a different rent exempt reserve due to data size changes
    // 2. The destination account being prefunded, which would lower the minimum split amount
    let destination_minimum_balance =
        destination_rent_exempt_reserve.saturating_add(additional_required_lamports);
    let destination_balance_deficit =
        destination_minimum_balance.saturating_sub(destination_lamports);
    if lamports < destination_balance_deficit {
        return Err(InstructionError::InsufficientFunds);
    }

    Ok(ValidatedSplitInfo {
        source_remaining_balance,
        destination_rent_exempt_reserve,
    })
}

#[derive(Clone, Debug, PartialEq)]
enum MergeKind {
    Inactive(Meta, u64, StakeFlags),
    ActivationEpoch(Meta, Stake, StakeFlags),
    FullyActive(Meta, Stake),
}

impl MergeKind {
    fn meta(&self) -> &Meta {
        match self {
            Self::Inactive(meta, _, _) => meta,
            Self::ActivationEpoch(meta, _, _) => meta,
            Self::FullyActive(meta, _) => meta,
        }
    }

    fn active_stake(&self) -> Option<&Stake> {
        match self {
            Self::Inactive(_, _, _) => None,
            Self::ActivationEpoch(_, stake, _) => Some(stake),
            Self::FullyActive(_, stake) => Some(stake),
        }
    }

    fn get_if_mergeable(
        invoke_context: &InvokeContext,
        stake_state: &StakeStateV2,
        stake_lamports: u64,
        clock: &Clock,
        stake_history: &StakeHistory,
    ) -> Result<Self, InstructionError> {
        match stake_state {
            StakeStateV2::Stake(meta, stake, stake_flags) => {
                // stake must not be in a transient state. Transient here meaning
                // activating or deactivating with non-zero effective stake.
                let status = stake.delegation.stake_activating_and_deactivating(
                    clock.epoch,
                    stake_history,
                    new_warmup_cooldown_rate_epoch(invoke_context),
                );

                match (status.effective, status.activating, status.deactivating) {
                    (0, 0, 0) => Ok(Self::Inactive(*meta, stake_lamports, *stake_flags)),
                    (0, _, _) => Ok(Self::ActivationEpoch(*meta, *stake, *stake_flags)),
                    (_, 0, 0) => Ok(Self::FullyActive(*meta, *stake)),
                    _ => {
                        let err = StakeError::MergeTransientStake;
                        ic_msg!(invoke_context, "{}", err);
                        Err(err.into())
                    }
                }
            }
            StakeStateV2::Initialized(meta) => {
                Ok(Self::Inactive(*meta, stake_lamports, StakeFlags::empty()))
            }
            _ => Err(InstructionError::InvalidAccountData),
        }
    }

    fn metas_can_merge(
        invoke_context: &InvokeContext,
        stake: &Meta,
        source: &Meta,
        clock: &Clock,
    ) -> Result<(), InstructionError> {
        // lockups may mismatch so long as both have expired
        let can_merge_lockups = stake.lockup == source.lockup
            || (!stake.lockup.is_in_force(clock, None) && !source.lockup.is_in_force(clock, None));
        // `rent_exempt_reserve` has no bearing on the mergeability of accounts,
        // as the source account will be culled by runtime once the operation
        // succeeds. Considering it here would needlessly prevent merging stake
        // accounts with differing data lengths, which already exist in the wild
        // due to an SDK bug
        if stake.authorized == source.authorized && can_merge_lockups {
            Ok(())
        } else {
            ic_msg!(invoke_context, "Unable to merge due to metadata mismatch");
            Err(StakeError::MergeMismatch.into())
        }
    }

    fn active_delegations_can_merge(
        invoke_context: &InvokeContext,
        stake: &Delegation,
        source: &Delegation,
    ) -> Result<(), InstructionError> {
        if stake.voter_pubkey != source.voter_pubkey {
            ic_msg!(invoke_context, "Unable to merge due to voter mismatch");
            Err(StakeError::MergeMismatch.into())
        } else if stake.deactivation_epoch == Epoch::MAX && source.deactivation_epoch == Epoch::MAX
        {
            Ok(())
        } else {
            ic_msg!(invoke_context, "Unable to merge due to stake deactivation");
            Err(StakeError::MergeMismatch.into())
        }
    }

    fn merge(
        self,
        invoke_context: &InvokeContext,
        source: Self,
        clock: &Clock,
    ) -> Result<Option<StakeStateV2>, InstructionError> {
        Self::metas_can_merge(invoke_context, self.meta(), source.meta(), clock)?;
        self.active_stake()
            .zip(source.active_stake())
            .map(|(stake, source)| {
                Self::active_delegations_can_merge(
                    invoke_context,
                    &stake.delegation,
                    &source.delegation,
                )
            })
            .unwrap_or(Ok(()))?;
        let merged_state = match (self, source) {
            (Self::Inactive(_, _, _), Self::Inactive(_, _, _)) => None,
            (Self::Inactive(_, _, _), Self::ActivationEpoch(_, _, _)) => None,
            (
                Self::ActivationEpoch(meta, mut stake, stake_flags),
                Self::Inactive(_, source_lamports, source_stake_flags),
            ) => {
                stake.delegation.stake = checked_add(stake.delegation.stake, source_lamports)?;
                Some(StakeStateV2::Stake(
                    meta,
                    stake,
                    stake_flags.union(source_stake_flags),
                ))
            }
            (
                Self::ActivationEpoch(meta, mut stake, stake_flags),
                Self::ActivationEpoch(source_meta, source_stake, source_stake_flags),
            ) => {
                let source_lamports = checked_add(
                    source_meta.rent_exempt_reserve,
                    source_stake.delegation.stake,
                )?;
                merge_delegation_stake_and_credits_observed(
                    &mut stake,
                    source_lamports,
                    source_stake.credits_observed,
                )?;
                Some(StakeStateV2::Stake(
                    meta,
                    stake,
                    stake_flags.union(source_stake_flags),
                ))
            }
            (Self::FullyActive(meta, mut stake), Self::FullyActive(_, source_stake)) => {
                // Don't stake the source account's `rent_exempt_reserve` to
                // protect against the magic activation loophole. It will
                // instead be moved into the destination account as extra,
                // withdrawable `lamports`
                merge_delegation_stake_and_credits_observed(
                    &mut stake,
                    source_stake.delegation.stake,
                    source_stake.credits_observed,
                )?;
                Some(StakeStateV2::Stake(meta, stake, StakeFlags::empty()))
            }
            _ => return Err(StakeError::MergeMismatch.into()),
        };
        Ok(merged_state)
    }
}

fn merge_delegation_stake_and_credits_observed(
    stake: &mut Stake,
    absorbed_lamports: u64,
    absorbed_credits_observed: u64,
) -> Result<(), InstructionError> {
    stake.credits_observed =
        stake_weighted_credits_observed(stake, absorbed_lamports, absorbed_credits_observed)
            .ok_or(InstructionError::ArithmeticOverflow)?;
    stake.delegation.stake = checked_add(stake.delegation.stake, absorbed_lamports)?;
    Ok(())
}

/// Calculate the effective credits observed for two stakes when merging
///
/// When merging two `ActivationEpoch` or `FullyActive` stakes, the credits
/// observed of the merged stake is the weighted average of the two stakes'
/// credits observed.
///
/// This is because we can derive the effective credits_observed by reversing the staking
/// rewards equation, _while keeping the rewards unchanged after merge (i.e. strong
/// requirement)_, like below:
///
/// a(N) => account, r => rewards, s => stake, c => credits:
/// assume:
///   a3 = merge(a1, a2)
/// then:
///   a3.s = a1.s + a2.s
///
/// Next, given:
///   aN.r = aN.c * aN.s (for every N)
/// finally:
///        a3.r = a1.r + a2.r
/// a3.c * a3.s = a1.c * a1.s + a2.c * a2.s
///        a3.c = (a1.c * a1.s + a2.c * a2.s) / (a1.s + a2.s)     // QED
///
/// (For this discussion, we omitted irrelevant variables, including distance
///  calculation against vote_account and point indirection.)
fn stake_weighted_credits_observed(
    stake: &Stake,
    absorbed_lamports: u64,
    absorbed_credits_observed: u64,
) -> Option<u64> {
    if stake.credits_observed == absorbed_credits_observed {
        Some(stake.credits_observed)
    } else {
        let total_stake = u128::from(stake.delegation.stake.checked_add(absorbed_lamports)?);
        let stake_weighted_credits =
            u128::from(stake.credits_observed).checked_mul(u128::from(stake.delegation.stake))?;
        let absorbed_weighted_credits =
            u128::from(absorbed_credits_observed).checked_mul(u128::from(absorbed_lamports))?;
        // Discard fractional credits as a merge side-effect friction by taking
        // the ceiling, done by adding `denominator - 1` to the numerator.
        let total_weighted_credits = stake_weighted_credits
            .checked_add(absorbed_weighted_credits)?
            .checked_add(total_stake)?
            .checked_sub(1)?;
        u64::try_from(total_weighted_credits.checked_div(total_stake)?).ok()
    }
}

pub type RewriteStakeStatus = (&'static str, (u64, u64), (u64, u64));

// utility function, used by runtime::Stakes, tests
pub fn new_stake_history_entry<'a, I>(
    epoch: Epoch,
    stakes: I,
    history: &StakeHistory,
    new_rate_activation_epoch: Option<Epoch>,
) -> StakeHistoryEntry
where
    I: Iterator<Item = &'a Delegation>,
{
    stakes.fold(StakeHistoryEntry::default(), |sum, stake| {
        sum + stake.stake_activating_and_deactivating(epoch, history, new_rate_activation_epoch)
    })
}

// utility function, used by tests
pub fn create_stake_history_from_delegations(
    bootstrap: Option<u64>,
    epochs: std::ops::Range<Epoch>,
    delegations: &[Delegation],
    new_rate_activation_epoch: Option<Epoch>,
) -> StakeHistory {
    let mut stake_history = StakeHistory::default();

    let bootstrap_delegation = if let Some(bootstrap) = bootstrap {
        vec![Delegation {
            activation_epoch: std::u64::MAX,
            stake: bootstrap,
            ..Delegation::default()
        }]
    } else {
        vec![]
    };

    for epoch in epochs {
        let entry = new_stake_history_entry(
            epoch,
            delegations.iter().chain(bootstrap_delegation.iter()),
            &stake_history,
            new_rate_activation_epoch,
        );
        stake_history.add(epoch, entry);
    }

    stake_history
}

// genesis investor accounts
pub fn create_lockup_stake_account(
    authorized: &Authorized,
    lockup: &Lockup,
    rent: &Rent,
    lamports: u64,
) -> AccountSharedData {
    let mut stake_account = AccountSharedData::new(lamports, StakeStateV2::size_of(), &id());

    let rent_exempt_reserve = rent.minimum_balance(stake_account.data().len());
    assert!(
        lamports >= rent_exempt_reserve,
        "lamports: {lamports} is less than rent_exempt_reserve {rent_exempt_reserve}"
    );

    stake_account
        .set_state(&StakeStateV2::Initialized(Meta {
            authorized: *authorized,
            lockup: *lockup,
            rent_exempt_reserve,
        }))
        .expect("set_state");

    stake_account
}

// utility function, used by Bank, tests, genesis for bootstrap
pub fn create_account(
    authorized: &Pubkey,
    voter_pubkey: &Pubkey,
    vote_account: &AccountSharedData,
    rent: &Rent,
    lamports: u64,
) -> AccountSharedData {
    do_create_account(
        authorized,
        voter_pubkey,
        vote_account,
        rent,
        lamports,
        Epoch::MAX,
    )
}

// utility function, used by tests
pub fn create_account_with_activation_epoch(
    authorized: &Pubkey,
    voter_pubkey: &Pubkey,
    vote_account: &AccountSharedData,
    rent: &Rent,
    lamports: u64,
    activation_epoch: Epoch,
) -> AccountSharedData {
    do_create_account(
        authorized,
        voter_pubkey,
        vote_account,
        rent,
        lamports,
        activation_epoch,
    )
}

fn do_create_account(
    authorized: &Pubkey,
    voter_pubkey: &Pubkey,
    vote_account: &AccountSharedData,
    rent: &Rent,
    lamports: u64,
    activation_epoch: Epoch,
) -> AccountSharedData {
    let mut stake_account = AccountSharedData::new(lamports, StakeStateV2::size_of(), &id());

    let vote_state = vote_state::from(vote_account).expect("vote_state");

    let rent_exempt_reserve = rent.minimum_balance(stake_account.data().len());

    stake_account
        .set_state(&StakeStateV2::Stake(
            Meta {
                authorized: Authorized::auto(authorized),
                rent_exempt_reserve,
                ..Meta::default()
            },
            new_stake(
                lamports - rent_exempt_reserve, // underflow is an error, is basically: assert!(lamports > rent_exempt_reserve);
                voter_pubkey,
                &vote_state,
                activation_epoch,
            ),
            StakeFlags::empty(),
        ))
        .expect("set_state");

    stake_account
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        proptest::prelude::*,
        solana_program_runtime::with_mock_invoke_context,
        solana_sdk::{
            account::{create_account_shared_data_for_test, AccountSharedData},
            epoch_schedule::EpochSchedule,
            pubkey::Pubkey,
            stake::state::warmup_cooldown_rate,
            sysvar::{epoch_schedule, SysvarId},
        },
        test_case::test_case,
    };

    #[test]
    fn test_authorized_authorize() {
        let staker = solana_sdk::pubkey::new_rand();
        let mut authorized = Authorized::auto(&staker);
        let mut signers = HashSet::new();
        assert_eq!(
            authorized.authorize(&signers, &staker, StakeAuthorize::Staker, None),
            Err(InstructionError::MissingRequiredSignature)
        );
        signers.insert(staker);
        assert_eq!(
            authorized.authorize(&signers, &staker, StakeAuthorize::Staker, None),
            Ok(())
        );
    }

    #[test]
    fn test_authorized_authorize_with_custodian() {
        let staker = solana_sdk::pubkey::new_rand();
        let custodian = solana_sdk::pubkey::new_rand();
        let invalid_custodian = solana_sdk::pubkey::new_rand();
        let mut authorized = Authorized::auto(&staker);
        let mut signers = HashSet::new();
        signers.insert(staker);

        let lockup = Lockup {
            epoch: 1,
            unix_timestamp: 1,
            custodian,
        };
        let clock = Clock {
            epoch: 0,
            unix_timestamp: 0,
            ..Clock::default()
        };

        // No lockup, no custodian
        assert_eq!(
            authorized.authorize(
                &signers,
                &staker,
                StakeAuthorize::Withdrawer,
                Some((&Lockup::default(), &clock, None))
            ),
            Ok(())
        );

        // No lockup, invalid custodian not a signer
        assert_eq!(
            authorized.authorize(
                &signers,
                &staker,
                StakeAuthorize::Withdrawer,
                Some((&Lockup::default(), &clock, Some(&invalid_custodian)))
            ),
            Ok(()) // <== invalid custodian doesn't matter, there's no lockup
        );

        // Lockup active, invalid custodian not a signer
        assert_eq!(
            authorized.authorize(
                &signers,
                &staker,
                StakeAuthorize::Withdrawer,
                Some((&lockup, &clock, Some(&invalid_custodian)))
            ),
            Err(StakeError::CustodianSignatureMissing.into()),
        );

        signers.insert(invalid_custodian);

        // No lockup, invalid custodian is a signer
        assert_eq!(
            authorized.authorize(
                &signers,
                &staker,
                StakeAuthorize::Withdrawer,
                Some((&Lockup::default(), &clock, Some(&invalid_custodian)))
            ),
            Ok(()) // <== invalid custodian doesn't matter, there's no lockup
        );

        // Lockup active, invalid custodian is a signer
        signers.insert(invalid_custodian);
        assert_eq!(
            authorized.authorize(
                &signers,
                &staker,
                StakeAuthorize::Withdrawer,
                Some((&lockup, &clock, Some(&invalid_custodian)))
            ),
            Err(StakeError::LockupInForce.into()), // <== invalid custodian rejected
        );

        signers.remove(&invalid_custodian);

        // Lockup active, no custodian
        assert_eq!(
            authorized.authorize(
                &signers,
                &staker,
                StakeAuthorize::Withdrawer,
                Some((&lockup, &clock, None))
            ),
            Err(StakeError::CustodianMissing.into()),
        );

        // Lockup active, custodian not a signer
        assert_eq!(
            authorized.authorize(
                &signers,
                &staker,
                StakeAuthorize::Withdrawer,
                Some((&lockup, &clock, Some(&custodian)))
            ),
            Err(StakeError::CustodianSignatureMissing.into()),
        );

        // Lockup active, custodian is a signer
        signers.insert(custodian);
        assert_eq!(
            authorized.authorize(
                &signers,
                &staker,
                StakeAuthorize::Withdrawer,
                Some((&lockup, &clock, Some(&custodian)))
            ),
            Ok(())
        );
    }

    #[test]
    fn test_stake_state_stake_from_fail() {
        let mut stake_account = AccountSharedData::new(0, StakeStateV2::size_of(), &id());

        stake_account
            .set_state(&StakeStateV2::default())
            .expect("set_state");

        assert_eq!(stake_from(&stake_account), None);
    }

    #[test]
    fn test_stake_is_bootstrap() {
        assert!(Delegation {
            activation_epoch: std::u64::MAX,
            ..Delegation::default()
        }
        .is_bootstrap());
        assert!(!Delegation {
            activation_epoch: 0,
            ..Delegation::default()
        }
        .is_bootstrap());
    }

    #[test]
    fn test_stake_activating_and_deactivating() {
        let stake = Delegation {
            stake: 1_000,
            activation_epoch: 0, // activating at zero
            deactivation_epoch: 5,
            ..Delegation::default()
        };

        // save this off so stake.config.warmup_rate changes don't break this test
        let increment = (1_000_f64 * warmup_cooldown_rate(0, None)) as u64;

        let mut stake_history = StakeHistory::default();
        // assert that this stake follows step function if there's no history
        assert_eq!(
            stake.stake_activating_and_deactivating(stake.activation_epoch, &stake_history, None),
            StakeActivationStatus::with_effective_and_activating(0, stake.stake),
        );
        for epoch in stake.activation_epoch + 1..stake.deactivation_epoch {
            assert_eq!(
                stake.stake_activating_and_deactivating(epoch, &stake_history, None),
                StakeActivationStatus::with_effective(stake.stake),
            );
        }
        // assert that this stake is full deactivating
        assert_eq!(
            stake.stake_activating_and_deactivating(stake.deactivation_epoch, &stake_history, None),
            StakeActivationStatus::with_deactivating(stake.stake),
        );
        // assert that this stake is fully deactivated if there's no history
        assert_eq!(
            stake.stake_activating_and_deactivating(
                stake.deactivation_epoch + 1,
                &stake_history,
                None
            ),
            StakeActivationStatus::default(),
        );

        stake_history.add(
            0u64, // entry for zero doesn't have my activating amount
            StakeHistoryEntry {
                effective: 1_000,
                ..StakeHistoryEntry::default()
            },
        );
        // assert that this stake is broken, because above setup is broken
        assert_eq!(
            stake.stake_activating_and_deactivating(1, &stake_history, None),
            StakeActivationStatus::with_effective_and_activating(0, stake.stake),
        );

        stake_history.add(
            0u64, // entry for zero has my activating amount
            StakeHistoryEntry {
                effective: 1_000,
                activating: 1_000,
                ..StakeHistoryEntry::default()
            },
            // no entry for 1, so this stake gets shorted
        );
        // assert that this stake is broken, because above setup is broken
        assert_eq!(
            stake.stake_activating_and_deactivating(2, &stake_history, None),
            StakeActivationStatus::with_effective_and_activating(
                increment,
                stake.stake - increment
            ),
        );

        // start over, test deactivation edge cases
        let mut stake_history = StakeHistory::default();

        stake_history.add(
            stake.deactivation_epoch, // entry for zero doesn't have my de-activating amount
            StakeHistoryEntry {
                effective: 1_000,
                ..StakeHistoryEntry::default()
            },
        );
        // assert that this stake is broken, because above setup is broken
        assert_eq!(
            stake.stake_activating_and_deactivating(
                stake.deactivation_epoch + 1,
                &stake_history,
                None,
            ),
            StakeActivationStatus::with_deactivating(stake.stake),
        );

        // put in my initial deactivating amount, but don't put in an entry for next
        stake_history.add(
            stake.deactivation_epoch, // entry for zero has my de-activating amount
            StakeHistoryEntry {
                effective: 1_000,
                deactivating: 1_000,
                ..StakeHistoryEntry::default()
            },
        );
        // assert that this stake is broken, because above setup is broken
        assert_eq!(
            stake.stake_activating_and_deactivating(
                stake.deactivation_epoch + 2,
                &stake_history,
                None,
            ),
            // hung, should be lower
            StakeActivationStatus::with_deactivating(stake.stake - increment),
        );
    }

    mod same_epoch_activation_then_deactivation {
        use super::*;

        enum OldDeactivationBehavior {
            Stuck,
            Slow,
        }

        fn do_test(
            old_behavior: OldDeactivationBehavior,
            expected_stakes: &[StakeActivationStatus],
        ) {
            let cluster_stake = 1_000;
            let activating_stake = 10_000;
            let some_stake = 700;
            let some_epoch = 0;

            let stake = Delegation {
                stake: some_stake,
                activation_epoch: some_epoch,
                deactivation_epoch: some_epoch,
                ..Delegation::default()
            };

            let mut stake_history = StakeHistory::default();
            let cluster_deactivation_at_stake_modified_epoch = match old_behavior {
                OldDeactivationBehavior::Stuck => 0,
                OldDeactivationBehavior::Slow => 1000,
            };

            let stake_history_entries = vec![
                (
                    cluster_stake,
                    activating_stake,
                    cluster_deactivation_at_stake_modified_epoch,
                ),
                (cluster_stake, activating_stake, 1000),
                (cluster_stake, activating_stake, 1000),
                (cluster_stake, activating_stake, 100),
                (cluster_stake, activating_stake, 100),
                (cluster_stake, activating_stake, 100),
                (cluster_stake, activating_stake, 100),
            ];

            for (epoch, (effective, activating, deactivating)) in
                stake_history_entries.into_iter().enumerate()
            {
                stake_history.add(
                    epoch as Epoch,
                    StakeHistoryEntry {
                        effective,
                        activating,
                        deactivating,
                    },
                );
            }

            assert_eq!(
                expected_stakes,
                (0..expected_stakes.len())
                    .map(|epoch| stake.stake_activating_and_deactivating(
                        epoch as u64,
                        &stake_history,
                        None,
                    ))
                    .collect::<Vec<_>>()
            );
        }

        #[test]
        fn test_new_behavior_previously_slow() {
            // any stake accounts activated and deactivated at the same epoch
            // shouldn't been activated (then deactivated) at all!

            do_test(
                OldDeactivationBehavior::Slow,
                &[
                    StakeActivationStatus::default(),
                    StakeActivationStatus::default(),
                    StakeActivationStatus::default(),
                    StakeActivationStatus::default(),
                    StakeActivationStatus::default(),
                    StakeActivationStatus::default(),
                    StakeActivationStatus::default(),
                ],
            );
        }

        #[test]
        fn test_new_behavior_previously_stuck() {
            // any stake accounts activated and deactivated at the same epoch
            // shouldn't been activated (then deactivated) at all!

            do_test(
                OldDeactivationBehavior::Stuck,
                &[
                    StakeActivationStatus::default(),
                    StakeActivationStatus::default(),
                    StakeActivationStatus::default(),
                    StakeActivationStatus::default(),
                    StakeActivationStatus::default(),
                    StakeActivationStatus::default(),
                    StakeActivationStatus::default(),
                ],
            );
        }
    }

    #[test]
    fn test_inflation_and_slashing_with_activating_and_deactivating_stake() {
        // some really boring delegation and stake_history setup
        let (delegated_stake, mut stake, stake_history) = {
            let cluster_stake = 1_000;
            let delegated_stake = 700;

            let stake = Delegation {
                stake: delegated_stake,
                activation_epoch: 0,
                deactivation_epoch: 4,
                ..Delegation::default()
            };

            let mut stake_history = StakeHistory::default();
            stake_history.add(
                0,
                StakeHistoryEntry {
                    effective: cluster_stake,
                    activating: delegated_stake,
                    ..StakeHistoryEntry::default()
                },
            );
            let newly_effective_at_epoch1 = (cluster_stake as f64 * 0.25) as u64;
            assert_eq!(newly_effective_at_epoch1, 250);
            stake_history.add(
                1,
                StakeHistoryEntry {
                    effective: cluster_stake + newly_effective_at_epoch1,
                    activating: delegated_stake - newly_effective_at_epoch1,
                    ..StakeHistoryEntry::default()
                },
            );
            let newly_effective_at_epoch2 =
                ((cluster_stake + newly_effective_at_epoch1) as f64 * 0.25) as u64;
            assert_eq!(newly_effective_at_epoch2, 312);
            stake_history.add(
                2,
                StakeHistoryEntry {
                    effective: cluster_stake
                        + newly_effective_at_epoch1
                        + newly_effective_at_epoch2,
                    activating: delegated_stake
                        - newly_effective_at_epoch1
                        - newly_effective_at_epoch2,
                    ..StakeHistoryEntry::default()
                },
            );
            stake_history.add(
                3,
                StakeHistoryEntry {
                    effective: cluster_stake + delegated_stake,
                    ..StakeHistoryEntry::default()
                },
            );
            stake_history.add(
                4,
                StakeHistoryEntry {
                    effective: cluster_stake + delegated_stake,
                    deactivating: delegated_stake,
                    ..StakeHistoryEntry::default()
                },
            );
            let newly_not_effective_stake_at_epoch5 =
                ((cluster_stake + delegated_stake) as f64 * 0.25) as u64;
            assert_eq!(newly_not_effective_stake_at_epoch5, 425);
            stake_history.add(
                5,
                StakeHistoryEntry {
                    effective: cluster_stake + delegated_stake
                        - newly_not_effective_stake_at_epoch5,
                    deactivating: delegated_stake - newly_not_effective_stake_at_epoch5,
                    ..StakeHistoryEntry::default()
                },
            );

            (delegated_stake, stake, stake_history)
        };

        // helper closures
        let calculate_each_staking_status = |stake: &Delegation, epoch_count: usize| -> Vec<_> {
            (0..epoch_count)
                .map(|epoch| {
                    stake.stake_activating_and_deactivating(epoch as u64, &stake_history, None)
                })
                .collect::<Vec<_>>()
        };
        let adjust_staking_status = |rate: f64, status: &[StakeActivationStatus]| {
            status
                .iter()
                .map(|entry| StakeActivationStatus {
                    effective: (entry.effective as f64 * rate) as u64,
                    activating: (entry.activating as f64 * rate) as u64,
                    deactivating: (entry.deactivating as f64 * rate) as u64,
                })
                .collect::<Vec<_>>()
        };

        let expected_staking_status_transition = vec![
            StakeActivationStatus::with_effective_and_activating(0, 700),
            StakeActivationStatus::with_effective_and_activating(250, 450),
            StakeActivationStatus::with_effective_and_activating(562, 138),
            StakeActivationStatus::with_effective(700),
            StakeActivationStatus::with_deactivating(700),
            StakeActivationStatus::with_deactivating(275),
            StakeActivationStatus::default(),
        ];
        let expected_staking_status_transition_base = vec![
            StakeActivationStatus::with_effective_and_activating(0, 700),
            StakeActivationStatus::with_effective_and_activating(250, 450),
            StakeActivationStatus::with_effective_and_activating(562, 138 + 1), // +1 is needed for rounding
            StakeActivationStatus::with_effective(700),
            StakeActivationStatus::with_deactivating(700),
            StakeActivationStatus::with_deactivating(275 + 1), // +1 is needed for rounding
            StakeActivationStatus::default(),
        ];

        // normal stake activating and deactivating transition test, just in case
        assert_eq!(
            expected_staking_status_transition,
            calculate_each_staking_status(&stake, expected_staking_status_transition.len())
        );

        // 10% inflation rewards assuming some sizable epochs passed!
        let rate = 1.10;
        stake.stake = (delegated_stake as f64 * rate) as u64;
        let expected_staking_status_transition =
            adjust_staking_status(rate, &expected_staking_status_transition_base);

        assert_eq!(
            expected_staking_status_transition,
            calculate_each_staking_status(&stake, expected_staking_status_transition_base.len()),
        );

        // 50% slashing!!!
        let rate = 0.5;
        stake.stake = (delegated_stake as f64 * rate) as u64;
        let expected_staking_status_transition =
            adjust_staking_status(rate, &expected_staking_status_transition_base);

        assert_eq!(
            expected_staking_status_transition,
            calculate_each_staking_status(&stake, expected_staking_status_transition_base.len()),
        );
    }

    #[test]
    fn test_stop_activating_after_deactivation() {
        let stake = Delegation {
            stake: 1_000,
            activation_epoch: 0,
            deactivation_epoch: 3,
            ..Delegation::default()
        };

        let base_stake = 1_000;
        let mut stake_history = StakeHistory::default();
        let mut effective = base_stake;
        let other_activation = 100;
        let mut other_activations = vec![0];

        // Build a stake history where the test staker always consumes all of the available warm
        // up and cool down stake. However, simulate other stakers beginning to activate during
        // the test staker's deactivation.
        for epoch in 0..=stake.deactivation_epoch + 1 {
            let (activating, deactivating) = if epoch < stake.deactivation_epoch {
                (stake.stake + base_stake - effective, 0)
            } else {
                let other_activation_sum: u64 = other_activations.iter().sum();
                let deactivating = effective - base_stake - other_activation_sum;
                (other_activation, deactivating)
            };

            stake_history.add(
                epoch,
                StakeHistoryEntry {
                    effective,
                    activating,
                    deactivating,
                },
            );

            let effective_rate_limited = (effective as f64 * warmup_cooldown_rate(0, None)) as u64;
            if epoch < stake.deactivation_epoch {
                effective += effective_rate_limited.min(activating);
                other_activations.push(0);
            } else {
                effective -= effective_rate_limited.min(deactivating);
                effective += other_activation;
                other_activations.push(other_activation);
            }
        }

        for epoch in 0..=stake.deactivation_epoch + 1 {
            let history = stake_history.get(epoch).unwrap();
            let other_activations: u64 = other_activations[..=epoch as usize].iter().sum();
            let expected_stake = history.effective - base_stake - other_activations;
            let (expected_activating, expected_deactivating) = if epoch < stake.deactivation_epoch {
                (history.activating, 0)
            } else {
                (0, history.deactivating)
            };
            assert_eq!(
                stake.stake_activating_and_deactivating(epoch, &stake_history, None),
                StakeActivationStatus {
                    effective: expected_stake,
                    activating: expected_activating,
                    deactivating: expected_deactivating,
                },
            );
        }
    }

    #[test]
    fn test_stake_warmup_cooldown_sub_integer_moves() {
        let delegations = [Delegation {
            stake: 2,
            activation_epoch: 0, // activating at zero
            deactivation_epoch: 5,
            ..Delegation::default()
        }];
        // give 2 epochs of cooldown
        let epochs = 7;
        // make boostrap stake smaller than warmup so warmup/cooldownn
        //  increment is always smaller than 1
        let bootstrap = (warmup_cooldown_rate(0, None) * 100.0 / 2.0) as u64;
        let stake_history =
            create_stake_history_from_delegations(Some(bootstrap), 0..epochs, &delegations, None);
        let mut max_stake = 0;
        let mut min_stake = 2;

        for epoch in 0..epochs {
            let stake = delegations
                .iter()
                .map(|delegation| delegation.stake(epoch, &stake_history, None))
                .sum::<u64>();
            max_stake = max_stake.max(stake);
            min_stake = min_stake.min(stake);
        }
        assert_eq!(max_stake, 2);
        assert_eq!(min_stake, 0);
    }

    #[test_case(None ; "old rate")]
    #[test_case(Some(1) ; "new rate activated in epoch 1")]
    #[test_case(Some(10) ; "new rate activated in epoch 10")]
    #[test_case(Some(30) ; "new rate activated in epoch 30")]
    #[test_case(Some(50) ; "new rate activated in epoch 50")]
    #[test_case(Some(60) ; "new rate activated in epoch 60")]
    fn test_stake_warmup_cooldown(new_rate_activation_epoch: Option<Epoch>) {
        let delegations = [
            Delegation {
                // never deactivates
                stake: 1_000,
                activation_epoch: std::u64::MAX,
                ..Delegation::default()
            },
            Delegation {
                stake: 1_000,
                activation_epoch: 0,
                deactivation_epoch: 9,
                ..Delegation::default()
            },
            Delegation {
                stake: 1_000,
                activation_epoch: 1,
                deactivation_epoch: 6,
                ..Delegation::default()
            },
            Delegation {
                stake: 1_000,
                activation_epoch: 2,
                deactivation_epoch: 5,
                ..Delegation::default()
            },
            Delegation {
                stake: 1_000,
                activation_epoch: 2,
                deactivation_epoch: 4,
                ..Delegation::default()
            },
            Delegation {
                stake: 1_000,
                activation_epoch: 4,
                deactivation_epoch: 4,
                ..Delegation::default()
            },
        ];
        // chosen to ensure that the last activated stake (at 4) finishes
        //  warming up and cooling down
        //  a stake takes 2.0f64.log(1.0 + STAKE_WARMUP_RATE) epochs to warm up or cool down
        //  when all alone, but the above overlap a lot
        let epochs = 60;

        let stake_history = create_stake_history_from_delegations(
            None,
            0..epochs,
            &delegations,
            new_rate_activation_epoch,
        );

        let mut prev_total_effective_stake = delegations
            .iter()
            .map(|delegation| delegation.stake(0, &stake_history, new_rate_activation_epoch))
            .sum::<u64>();

        // uncomment and add ! for fun with graphing
        // eprintln("\n{:8} {:8} {:8}", "   epoch", "   total", "   delta");
        for epoch in 1..epochs {
            let total_effective_stake = delegations
                .iter()
                .map(|delegation| {
                    delegation.stake(epoch, &stake_history, new_rate_activation_epoch)
                })
                .sum::<u64>();

            let delta = if total_effective_stake > prev_total_effective_stake {
                total_effective_stake - prev_total_effective_stake
            } else {
                prev_total_effective_stake - total_effective_stake
            };

            // uncomment and add ! for fun with graphing
            // eprint("{:8} {:8} {:8} ", epoch, total_effective_stake, delta);
            // (0..(total_effective_stake as usize / (delegations.len() * 5))).for_each(|_| eprint("#"));
            // eprintln();

            assert!(
                delta
                    <= ((prev_total_effective_stake as f64
                        * warmup_cooldown_rate(epoch, new_rate_activation_epoch))
                        as u64)
                        .max(1)
            );

            prev_total_effective_stake = total_effective_stake;
        }
    }

    #[test]
    fn test_lockup_is_expired() {
        let custodian = solana_sdk::pubkey::new_rand();
        let lockup = Lockup {
            epoch: 1,
            unix_timestamp: 1,
            custodian,
        };
        // neither time
        assert!(lockup.is_in_force(
            &Clock {
                epoch: 0,
                unix_timestamp: 0,
                ..Clock::default()
            },
            None
        ));
        // not timestamp
        assert!(lockup.is_in_force(
            &Clock {
                epoch: 2,
                unix_timestamp: 0,
                ..Clock::default()
            },
            None
        ));
        // not epoch
        assert!(lockup.is_in_force(
            &Clock {
                epoch: 0,
                unix_timestamp: 2,
                ..Clock::default()
            },
            None
        ));
        // both, no custodian
        assert!(!lockup.is_in_force(
            &Clock {
                epoch: 1,
                unix_timestamp: 1,
                ..Clock::default()
            },
            None
        ));
        // neither, but custodian
        assert!(!lockup.is_in_force(
            &Clock {
                epoch: 0,
                unix_timestamp: 0,
                ..Clock::default()
            },
            Some(&custodian),
        ));
    }

    #[test]
    #[ignore]
    #[should_panic]
    fn test_dbg_stake_minimum_balance() {
        let minimum_balance = Rent::default().minimum_balance(StakeStateV2::size_of());
        panic!(
            "stake minimum_balance: {} lamports, {} SOL",
            minimum_balance,
            minimum_balance as f64 / solana_sdk::native_token::LAMPORTS_PER_SOL as f64
        );
    }

    #[test]
    fn test_things_can_merge() {
        with_mock_invoke_context!(invoke_context, transaction_context, Vec::new());
        let good_stake = Stake {
            credits_observed: 4242,
            delegation: Delegation {
                voter_pubkey: Pubkey::new_unique(),
                stake: 424242424242,
                activation_epoch: 42,
                ..Delegation::default()
            },
        };

        let identical = good_stake;
        assert!(MergeKind::active_delegations_can_merge(
            &invoke_context,
            &good_stake.delegation,
            &identical.delegation
        )
        .is_ok());

        let good_delegation = good_stake.delegation;
        let different_stake_ok = Delegation {
            stake: good_delegation.stake + 1,
            ..good_delegation
        };
        assert!(MergeKind::active_delegations_can_merge(
            &invoke_context,
            &good_delegation,
            &different_stake_ok
        )
        .is_ok());

        let different_activation_epoch_ok = Delegation {
            activation_epoch: good_delegation.activation_epoch + 1,
            ..good_delegation
        };
        assert!(MergeKind::active_delegations_can_merge(
            &invoke_context,
            &good_delegation,
            &different_activation_epoch_ok
        )
        .is_ok());

        let bad_voter = Delegation {
            voter_pubkey: Pubkey::new_unique(),
            ..good_delegation
        };
        assert!(MergeKind::active_delegations_can_merge(
            &invoke_context,
            &good_delegation,
            &bad_voter
        )
        .is_err());

        let bad_deactivation_epoch = Delegation {
            deactivation_epoch: 43,
            ..good_delegation
        };
        assert!(MergeKind::active_delegations_can_merge(
            &invoke_context,
            &good_delegation,
            &bad_deactivation_epoch
        )
        .is_err());
        assert!(MergeKind::active_delegations_can_merge(
            &invoke_context,
            &bad_deactivation_epoch,
            &good_delegation
        )
        .is_err());
    }

    #[test]
    fn test_metas_can_merge() {
        with_mock_invoke_context!(invoke_context, transaction_context, Vec::new());
        // Identical Metas can merge
        assert!(MergeKind::metas_can_merge(
            &invoke_context,
            &Meta::default(),
            &Meta::default(),
            &Clock::default()
        )
        .is_ok());

        let mismatched_rent_exempt_reserve_ok = Meta {
            rent_exempt_reserve: 42,
            ..Meta::default()
        };
        assert_ne!(
            mismatched_rent_exempt_reserve_ok.rent_exempt_reserve,
            Meta::default().rent_exempt_reserve,
        );
        assert!(MergeKind::metas_can_merge(
            &invoke_context,
            &Meta::default(),
            &mismatched_rent_exempt_reserve_ok,
            &Clock::default()
        )
        .is_ok());
        assert!(MergeKind::metas_can_merge(
            &invoke_context,
            &mismatched_rent_exempt_reserve_ok,
            &Meta::default(),
            &Clock::default()
        )
        .is_ok());

        let mismatched_authorized_fails = Meta {
            authorized: Authorized {
                staker: Pubkey::new_unique(),
                withdrawer: Pubkey::new_unique(),
            },
            ..Meta::default()
        };
        assert_ne!(
            mismatched_authorized_fails.authorized,
            Meta::default().authorized,
        );
        assert!(MergeKind::metas_can_merge(
            &invoke_context,
            &Meta::default(),
            &mismatched_authorized_fails,
            &Clock::default()
        )
        .is_err());
        assert!(MergeKind::metas_can_merge(
            &invoke_context,
            &mismatched_authorized_fails,
            &Meta::default(),
            &Clock::default()
        )
        .is_err());

        let lockup1_timestamp = 42;
        let lockup2_timestamp = 4242;
        let lockup1_epoch = 4;
        let lockup2_epoch = 42;
        let metas_with_lockup1 = Meta {
            lockup: Lockup {
                unix_timestamp: lockup1_timestamp,
                epoch: lockup1_epoch,
                custodian: Pubkey::new_unique(),
            },
            ..Meta::default()
        };
        let metas_with_lockup2 = Meta {
            lockup: Lockup {
                unix_timestamp: lockup2_timestamp,
                epoch: lockup2_epoch,
                custodian: Pubkey::new_unique(),
            },
            ..Meta::default()
        };

        // Mismatched lockups fail when both in force
        assert_ne!(metas_with_lockup1.lockup, Meta::default().lockup);
        assert!(MergeKind::metas_can_merge(
            &invoke_context,
            &metas_with_lockup1,
            &metas_with_lockup2,
            &Clock::default()
        )
        .is_err());
        assert!(MergeKind::metas_can_merge(
            &invoke_context,
            &metas_with_lockup2,
            &metas_with_lockup1,
            &Clock::default()
        )
        .is_err());

        let clock = Clock {
            epoch: lockup1_epoch + 1,
            unix_timestamp: lockup1_timestamp + 1,
            ..Clock::default()
        };

        // Mismatched lockups fail when either in force
        assert_ne!(metas_with_lockup1.lockup, Meta::default().lockup);
        assert!(MergeKind::metas_can_merge(
            &invoke_context,
            &metas_with_lockup1,
            &metas_with_lockup2,
            &clock
        )
        .is_err());
        assert!(MergeKind::metas_can_merge(
            &invoke_context,
            &metas_with_lockup2,
            &metas_with_lockup1,
            &clock
        )
        .is_err());

        let clock = Clock {
            epoch: lockup2_epoch + 1,
            unix_timestamp: lockup2_timestamp + 1,
            ..Clock::default()
        };

        // Mismatched lockups succeed when both expired
        assert_ne!(metas_with_lockup1.lockup, Meta::default().lockup);
        assert!(MergeKind::metas_can_merge(
            &invoke_context,
            &metas_with_lockup1,
            &metas_with_lockup2,
            &clock
        )
        .is_ok());
        assert!(MergeKind::metas_can_merge(
            &invoke_context,
            &metas_with_lockup2,
            &metas_with_lockup1,
            &clock
        )
        .is_ok());
    }

    #[test]
    fn test_merge_kind_get_if_mergeable() {
        let transaction_accounts = vec![(
            epoch_schedule::id(),
            create_account_shared_data_for_test(&EpochSchedule::default()),
        )];
        with_mock_invoke_context!(invoke_context, transaction_context, transaction_accounts);
        let authority_pubkey = Pubkey::new_unique();
        let initial_lamports = 4242424242;
        let rent = Rent::default();
        let rent_exempt_reserve = rent.minimum_balance(StakeStateV2::size_of());
        let stake_lamports = rent_exempt_reserve + initial_lamports;
        let new_rate_activation_epoch = Some(0);

        let meta = Meta {
            rent_exempt_reserve,
            ..Meta::auto(&authority_pubkey)
        };
        let mut stake_account = AccountSharedData::new_data_with_space(
            stake_lamports,
            &StakeStateV2::Uninitialized,
            StakeStateV2::size_of(),
            &id(),
        )
        .expect("stake_account");
        let mut clock = Clock::default();
        let mut stake_history = StakeHistory::default();

        // Uninitialized state fails
        assert_eq!(
            MergeKind::get_if_mergeable(
                &invoke_context,
                &stake_account.state().unwrap(),
                stake_account.lamports(),
                &clock,
                &stake_history
            )
            .unwrap_err(),
            InstructionError::InvalidAccountData
        );

        // RewardsPool state fails
        stake_account.set_state(&StakeStateV2::RewardsPool).unwrap();
        assert_eq!(
            MergeKind::get_if_mergeable(
                &invoke_context,
                &stake_account.state().unwrap(),
                stake_account.lamports(),
                &clock,
                &stake_history
            )
            .unwrap_err(),
            InstructionError::InvalidAccountData
        );

        // Initialized state succeeds
        stake_account
            .set_state(&StakeStateV2::Initialized(meta))
            .unwrap();
        assert_eq!(
            MergeKind::get_if_mergeable(
                &invoke_context,
                &stake_account.state().unwrap(),
                stake_account.lamports(),
                &clock,
                &stake_history
            )
            .unwrap(),
            MergeKind::Inactive(meta, stake_lamports, StakeFlags::empty())
        );

        clock.epoch = 0;
        let mut effective = 2 * initial_lamports;
        let mut activating = 0;
        let mut deactivating = 0;
        stake_history.add(
            clock.epoch,
            StakeHistoryEntry {
                effective,
                activating,
                deactivating,
            },
        );

        clock.epoch += 1;
        activating = initial_lamports;
        stake_history.add(
            clock.epoch,
            StakeHistoryEntry {
                effective,
                activating,
                deactivating,
            },
        );

        let stake = Stake {
            delegation: Delegation {
                stake: initial_lamports,
                activation_epoch: 1,
                deactivation_epoch: 9,
                ..Delegation::default()
            },
            ..Stake::default()
        };
        stake_account
            .set_state(&StakeStateV2::Stake(meta, stake, StakeFlags::empty()))
            .unwrap();
        // activation_epoch succeeds
        assert_eq!(
            MergeKind::get_if_mergeable(
                &invoke_context,
                &stake_account.state().unwrap(),
                stake_account.lamports(),
                &clock,
                &stake_history
            )
            .unwrap(),
            MergeKind::ActivationEpoch(meta, stake, StakeFlags::empty()),
        );

        // all paritially activated, transient epochs fail
        loop {
            clock.epoch += 1;
            let delta = activating.min(
                (effective as f64 * warmup_cooldown_rate(clock.epoch, new_rate_activation_epoch))
                    as u64,
            );
            effective += delta;
            activating -= delta;
            stake_history.add(
                clock.epoch,
                StakeHistoryEntry {
                    effective,
                    activating,
                    deactivating,
                },
            );
            if activating == 0 {
                break;
            }
            assert_eq!(
                MergeKind::get_if_mergeable(
                    &invoke_context,
                    &stake_account.state().unwrap(),
                    stake_account.lamports(),
                    &clock,
                    &stake_history
                )
                .unwrap_err(),
                InstructionError::from(StakeError::MergeTransientStake),
            );
        }

        // all epochs for which we're fully active succeed
        while clock.epoch < stake.delegation.deactivation_epoch - 1 {
            clock.epoch += 1;
            stake_history.add(
                clock.epoch,
                StakeHistoryEntry {
                    effective,
                    activating,
                    deactivating,
                },
            );
            assert_eq!(
                MergeKind::get_if_mergeable(
                    &invoke_context,
                    &stake_account.state().unwrap(),
                    stake_account.lamports(),
                    &clock,
                    &stake_history
                )
                .unwrap(),
                MergeKind::FullyActive(meta, stake),
            );
        }

        clock.epoch += 1;
        deactivating = stake.delegation.stake;
        stake_history.add(
            clock.epoch,
            StakeHistoryEntry {
                effective,
                activating,
                deactivating,
            },
        );
        // deactivation epoch fails, fully transient/deactivating
        assert_eq!(
            MergeKind::get_if_mergeable(
                &invoke_context,
                &stake_account.state().unwrap(),
                stake_account.lamports(),
                &clock,
                &stake_history
            )
            .unwrap_err(),
            InstructionError::from(StakeError::MergeTransientStake),
        );

        // all transient, deactivating epochs fail
        loop {
            clock.epoch += 1;
            let delta = deactivating.min(
                (effective as f64 * warmup_cooldown_rate(clock.epoch, new_rate_activation_epoch))
                    as u64,
            );
            effective -= delta;
            deactivating -= delta;
            stake_history.add(
                clock.epoch,
                StakeHistoryEntry {
                    effective,
                    activating,
                    deactivating,
                },
            );
            if deactivating == 0 {
                break;
            }
            assert_eq!(
                MergeKind::get_if_mergeable(
                    &invoke_context,
                    &stake_account.state().unwrap(),
                    stake_account.lamports(),
                    &clock,
                    &stake_history
                )
                .unwrap_err(),
                InstructionError::from(StakeError::MergeTransientStake),
            );
        }

        // first fully-deactivated epoch succeeds
        assert_eq!(
            MergeKind::get_if_mergeable(
                &invoke_context,
                &stake_account.state().unwrap(),
                stake_account.lamports(),
                &clock,
                &stake_history
            )
            .unwrap(),
            MergeKind::Inactive(meta, stake_lamports, StakeFlags::empty()),
        );
    }

    #[test]
    fn test_merge_kind_merge() {
        with_mock_invoke_context!(invoke_context, transaction_context, Vec::new());
        let clock = Clock::default();
        let lamports = 424242;
        let meta = Meta {
            rent_exempt_reserve: 42,
            ..Meta::default()
        };
        let stake = Stake {
            delegation: Delegation {
                stake: 4242,
                ..Delegation::default()
            },
            ..Stake::default()
        };
        let inactive = MergeKind::Inactive(Meta::default(), lamports, StakeFlags::empty());
        let activation_epoch = MergeKind::ActivationEpoch(meta, stake, StakeFlags::empty());
        let fully_active = MergeKind::FullyActive(meta, stake);

        assert_eq!(
            inactive
                .clone()
                .merge(&invoke_context, inactive.clone(), &clock)
                .unwrap(),
            None
        );
        assert_eq!(
            inactive
                .clone()
                .merge(&invoke_context, activation_epoch.clone(), &clock)
                .unwrap(),
            None
        );
        assert!(inactive
            .clone()
            .merge(&invoke_context, fully_active.clone(), &clock)
            .is_err());
        assert!(activation_epoch
            .clone()
            .merge(&invoke_context, fully_active.clone(), &clock)
            .is_err());
        assert!(fully_active
            .clone()
            .merge(&invoke_context, inactive.clone(), &clock)
            .is_err());
        assert!(fully_active
            .clone()
            .merge(&invoke_context, activation_epoch.clone(), &clock)
            .is_err());

        let new_state = activation_epoch
            .clone()
            .merge(&invoke_context, inactive, &clock)
            .unwrap()
            .unwrap();
        let delegation = new_state.delegation().unwrap();
        assert_eq!(delegation.stake, stake.delegation.stake + lamports);

        let new_state = activation_epoch
            .clone()
            .merge(&invoke_context, activation_epoch, &clock)
            .unwrap()
            .unwrap();
        let delegation = new_state.delegation().unwrap();
        assert_eq!(
            delegation.stake,
            2 * stake.delegation.stake + meta.rent_exempt_reserve
        );

        let new_state = fully_active
            .clone()
            .merge(&invoke_context, fully_active, &clock)
            .unwrap()
            .unwrap();
        let delegation = new_state.delegation().unwrap();
        assert_eq!(delegation.stake, 2 * stake.delegation.stake);
    }

    #[test]
    fn test_active_stake_merge() {
        let transaction_accounts = vec![(
            Rent::id(),
            create_account_shared_data_for_test(&Rent::default()),
        )];
        with_mock_invoke_context!(invoke_context, transaction_context, transaction_accounts);
        let clock = Clock::default();
        let delegation_a = 4_242_424_242u64;
        let delegation_b = 6_200_000_000u64;
        let credits_a = 124_521_000u64;
        let rent_exempt_reserve = 227_000_000u64;
        let meta = Meta {
            rent_exempt_reserve,
            ..Meta::default()
        };
        let stake_a = Stake {
            delegation: Delegation {
                stake: delegation_a,
                ..Delegation::default()
            },
            credits_observed: credits_a,
        };
        let stake_b = Stake {
            delegation: Delegation {
                stake: delegation_b,
                ..Delegation::default()
            },
            credits_observed: credits_a,
        };

        // activating stake merge, match credits observed
        let activation_epoch_a = MergeKind::ActivationEpoch(meta, stake_a, StakeFlags::empty());
        let activation_epoch_b = MergeKind::ActivationEpoch(meta, stake_b, StakeFlags::empty());
        let new_stake = activation_epoch_a
            .merge(&invoke_context, activation_epoch_b, &clock)
            .unwrap()
            .unwrap()
            .stake()
            .unwrap();
        assert_eq!(new_stake.credits_observed, credits_a);
        assert_eq!(
            new_stake.delegation.stake,
            delegation_a + delegation_b + rent_exempt_reserve
        );

        // active stake merge, match credits observed
        let fully_active_a = MergeKind::FullyActive(meta, stake_a);
        let fully_active_b = MergeKind::FullyActive(meta, stake_b);
        let new_stake = fully_active_a
            .merge(&invoke_context, fully_active_b, &clock)
            .unwrap()
            .unwrap()
            .stake()
            .unwrap();
        assert_eq!(new_stake.credits_observed, credits_a);
        assert_eq!(new_stake.delegation.stake, delegation_a + delegation_b);

        // activating stake merge, unmatched credits observed
        let credits_b = 125_124_521u64;
        let stake_b = Stake {
            delegation: Delegation {
                stake: delegation_b,
                ..Delegation::default()
            },
            credits_observed: credits_b,
        };
        let activation_epoch_a = MergeKind::ActivationEpoch(meta, stake_a, StakeFlags::empty());
        let activation_epoch_b = MergeKind::ActivationEpoch(meta, stake_b, StakeFlags::empty());
        let new_stake = activation_epoch_a
            .merge(&invoke_context, activation_epoch_b, &clock)
            .unwrap()
            .unwrap()
            .stake()
            .unwrap();
        assert_eq!(
            new_stake.credits_observed,
            (credits_a * delegation_a + credits_b * (delegation_b + rent_exempt_reserve))
                / (delegation_a + delegation_b + rent_exempt_reserve)
                + 1
        );
        assert_eq!(
            new_stake.delegation.stake,
            delegation_a + delegation_b + rent_exempt_reserve
        );

        // active stake merge, unmatched credits observed
        let fully_active_a = MergeKind::FullyActive(meta, stake_a);
        let fully_active_b = MergeKind::FullyActive(meta, stake_b);
        let new_stake = fully_active_a
            .merge(&invoke_context, fully_active_b, &clock)
            .unwrap()
            .unwrap()
            .stake()
            .unwrap();
        assert_eq!(
            new_stake.credits_observed,
            (credits_a * delegation_a + credits_b * delegation_b) / (delegation_a + delegation_b)
                + 1
        );
        assert_eq!(new_stake.delegation.stake, delegation_a + delegation_b);

        // active stake merge, unmatched credits observed, no need to ceiling the calculation
        let delegation = 1_000_000u64;
        let credits_a = 200_000_000u64;
        let credits_b = 100_000_000u64;
        let rent_exempt_reserve = 227_000_000u64;
        let meta = Meta {
            rent_exempt_reserve,
            ..Meta::default()
        };
        let stake_a = Stake {
            delegation: Delegation {
                stake: delegation,
                ..Delegation::default()
            },
            credits_observed: credits_a,
        };
        let stake_b = Stake {
            delegation: Delegation {
                stake: delegation,
                ..Delegation::default()
            },
            credits_observed: credits_b,
        };
        let fully_active_a = MergeKind::FullyActive(meta, stake_a);
        let fully_active_b = MergeKind::FullyActive(meta, stake_b);
        let new_stake = fully_active_a
            .merge(&invoke_context, fully_active_b, &clock)
            .unwrap()
            .unwrap()
            .stake()
            .unwrap();
        assert_eq!(
            new_stake.credits_observed,
            (credits_a * delegation + credits_b * delegation) / (delegation + delegation)
        );
        assert_eq!(new_stake.delegation.stake, delegation * 2);
    }

    prop_compose! {
        pub fn sum_within(max: u64)(total in 1..max)
            (intermediate in 1..total, total in Just(total))
            -> (u64, u64) {
                (intermediate, total - intermediate)
        }
    }

    proptest! {
        #[test]
        fn test_stake_weighted_credits_observed(
            (credits_a, credits_b) in sum_within(u64::MAX),
            (delegation_a, delegation_b) in sum_within(u64::MAX),
        ) {
            let stake = Stake {
                delegation: Delegation {
                    stake: delegation_a,
                    ..Delegation::default()
                },
                credits_observed: credits_a
            };
            let credits_observed = stake_weighted_credits_observed(
                &stake,
                delegation_b,
                credits_b,
            ).unwrap();

            // calculated credits observed should always be between the credits of a and b
            if credits_a < credits_b {
                assert!(credits_a < credits_observed);
                assert!(credits_observed <= credits_b);
            } else {
                assert!(credits_b <= credits_observed);
                assert!(credits_observed <= credits_a);
            }

            // the difference of the combined weighted credits and the separate weighted credits
            // should be 1 or 0
            let weighted_credits_total = credits_observed as u128 * (delegation_a + delegation_b) as u128;
            let weighted_credits_a = credits_a as u128 * delegation_a as u128;
            let weighted_credits_b = credits_b as u128 * delegation_b as u128;
            let raw_diff = weighted_credits_total - (weighted_credits_a + weighted_credits_b);
            let credits_observed_diff = raw_diff / (delegation_a + delegation_b) as u128;
            assert!(credits_observed_diff <= 1);
        }
    }
}
