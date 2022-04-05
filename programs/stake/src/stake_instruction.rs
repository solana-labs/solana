#[deprecated(
    since = "1.8.0",
    note = "Please use `solana_sdk::stake::instruction` or `solana_program::stake::instruction` instead"
)]
pub use solana_sdk::stake::instruction::*;
use {
    crate::{config, stake_state::StakeAccount},
    log::*,
    solana_program_runtime::{
        invoke_context::InvokeContext, sysvar_cache::get_sysvar_with_account_check,
    },
    solana_sdk::{
        feature_set,
        instruction::InstructionError,
        keyed_account::keyed_account_at_index,
        program_utils::limited_deserialize,
        stake::{
            instruction::StakeInstruction,
            program::id,
            state::{Authorized, Lockup},
        },
        sysvar::clock::Clock,
    },
};

pub fn process_instruction(
    first_instruction_account: usize,
    invoke_context: &mut InvokeContext,
) -> Result<(), InstructionError> {
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;
    let data = instruction_context.get_instruction_data();
    let keyed_accounts = invoke_context.get_keyed_accounts()?;

    trace!("process_instruction: {:?}", data);
    trace!("keyed_accounts: {:?}", keyed_accounts);

    let me = &keyed_account_at_index(keyed_accounts, first_instruction_account)?;
    if me.owner()? != id() {
        return Err(InstructionError::InvalidAccountOwner);
    }

    let signers = instruction_context.get_signers(transaction_context);
    match limited_deserialize(data)? {
        StakeInstruction::Initialize(authorized, lockup) => {
            let rent = get_sysvar_with_account_check::rent(invoke_context, instruction_context, 1)?;
            me.initialize(&authorized, &lockup, &rent, &invoke_context.feature_set)
        }
        StakeInstruction::Authorize(authorized_pubkey, stake_authorize) => {
            instruction_context.check_number_of_instruction_accounts(3)?;
            let require_custodian_for_locked_stake_authorize = invoke_context
                .feature_set
                .is_active(&feature_set::require_custodian_for_locked_stake_authorize::id());

            if require_custodian_for_locked_stake_authorize {
                let clock =
                    get_sysvar_with_account_check::clock(invoke_context, instruction_context, 1)?;
                let _current_authority =
                    keyed_account_at_index(keyed_accounts, first_instruction_account + 2)?;
                let custodian =
                    keyed_account_at_index(keyed_accounts, first_instruction_account + 3)
                        .ok()
                        .map(|ka| ka.unsigned_key());

                me.authorize(
                    &signers,
                    &authorized_pubkey,
                    stake_authorize,
                    require_custodian_for_locked_stake_authorize,
                    &clock,
                    custodian,
                )
            } else {
                me.authorize(
                    &signers,
                    &authorized_pubkey,
                    stake_authorize,
                    require_custodian_for_locked_stake_authorize,
                    &Clock::default(),
                    None,
                )
            }
        }
        StakeInstruction::AuthorizeWithSeed(args) => {
            instruction_context.check_number_of_instruction_accounts(2)?;
            let authority_base =
                keyed_account_at_index(keyed_accounts, first_instruction_account + 1)?;
            let require_custodian_for_locked_stake_authorize = invoke_context
                .feature_set
                .is_active(&feature_set::require_custodian_for_locked_stake_authorize::id());

            if require_custodian_for_locked_stake_authorize {
                let clock =
                    get_sysvar_with_account_check::clock(invoke_context, instruction_context, 2)?;
                let custodian =
                    keyed_account_at_index(keyed_accounts, first_instruction_account + 3)
                        .ok()
                        .map(|ka| ka.unsigned_key());

                me.authorize_with_seed(
                    authority_base,
                    &args.authority_seed,
                    &args.authority_owner,
                    &args.new_authorized_pubkey,
                    args.stake_authorize,
                    require_custodian_for_locked_stake_authorize,
                    &clock,
                    custodian,
                )
            } else {
                me.authorize_with_seed(
                    authority_base,
                    &args.authority_seed,
                    &args.authority_owner,
                    &args.new_authorized_pubkey,
                    args.stake_authorize,
                    require_custodian_for_locked_stake_authorize,
                    &Clock::default(),
                    None,
                )
            }
        }
        StakeInstruction::DelegateStake => {
            instruction_context.check_number_of_instruction_accounts(2)?;
            let vote = keyed_account_at_index(keyed_accounts, first_instruction_account + 1)?;
            let clock =
                get_sysvar_with_account_check::clock(invoke_context, instruction_context, 2)?;
            let stake_history = get_sysvar_with_account_check::stake_history(
                invoke_context,
                instruction_context,
                3,
            )?;
            instruction_context.check_number_of_instruction_accounts(5)?;
            let config_account =
                keyed_account_at_index(keyed_accounts, first_instruction_account + 4)?;
            if !config::check_id(config_account.unsigned_key()) {
                return Err(InstructionError::InvalidArgument);
            }
            let config = config::from(&*config_account.try_account_ref()?)
                .ok_or(InstructionError::InvalidArgument)?;
            me.delegate(vote, &clock, &stake_history, &config, &signers)
        }
        StakeInstruction::Split(lamports) => {
            instruction_context.check_number_of_instruction_accounts(2)?;
            let split_stake =
                &keyed_account_at_index(keyed_accounts, first_instruction_account + 1)?;
            me.split(invoke_context, lamports, split_stake, &signers)
        }
        StakeInstruction::Merge => {
            instruction_context.check_number_of_instruction_accounts(2)?;
            let source_stake =
                &keyed_account_at_index(keyed_accounts, first_instruction_account + 1)?;
            let clock =
                get_sysvar_with_account_check::clock(invoke_context, instruction_context, 2)?;
            let stake_history = get_sysvar_with_account_check::stake_history(
                invoke_context,
                instruction_context,
                3,
            )?;
            me.merge(
                invoke_context,
                source_stake,
                &clock,
                &stake_history,
                &signers,
            )
        }
        StakeInstruction::Withdraw(lamports) => {
            instruction_context.check_number_of_instruction_accounts(2)?;
            let to = &keyed_account_at_index(keyed_accounts, first_instruction_account + 1)?;
            let clock =
                get_sysvar_with_account_check::clock(invoke_context, instruction_context, 2)?;
            let stake_history = get_sysvar_with_account_check::stake_history(
                invoke_context,
                instruction_context,
                3,
            )?;
            instruction_context.check_number_of_instruction_accounts(5)?;
            me.withdraw(
                lamports,
                to,
                &clock,
                &stake_history,
                keyed_account_at_index(keyed_accounts, first_instruction_account + 4)?,
                keyed_account_at_index(keyed_accounts, first_instruction_account + 5).ok(),
                &invoke_context.feature_set,
            )
        }
        StakeInstruction::Deactivate => {
            let clock =
                get_sysvar_with_account_check::clock(invoke_context, instruction_context, 1)?;
            me.deactivate(&clock, &signers)
        }
        StakeInstruction::SetLockup(lockup) => {
            let clock = invoke_context.get_sysvar_cache().get_clock()?;
            me.set_lockup(&lockup, &signers, &clock)
        }
        StakeInstruction::InitializeChecked => {
            if invoke_context
                .feature_set
                .is_active(&feature_set::vote_stake_checked_instructions::id())
            {
                instruction_context.check_number_of_instruction_accounts(4)?;
                let authorized = Authorized {
                    staker: *keyed_account_at_index(keyed_accounts, first_instruction_account + 2)?
                        .unsigned_key(),
                    withdrawer: *keyed_account_at_index(
                        keyed_accounts,
                        first_instruction_account + 3,
                    )?
                    .signer_key()
                    .ok_or(InstructionError::MissingRequiredSignature)?,
                };

                let rent =
                    get_sysvar_with_account_check::rent(invoke_context, instruction_context, 1)?;
                me.initialize(
                    &authorized,
                    &Lockup::default(),
                    &rent,
                    &invoke_context.feature_set,
                )
            } else {
                Err(InstructionError::InvalidInstructionData)
            }
        }
        StakeInstruction::AuthorizeChecked(stake_authorize) => {
            if invoke_context
                .feature_set
                .is_active(&feature_set::vote_stake_checked_instructions::id())
            {
                let clock =
                    get_sysvar_with_account_check::clock(invoke_context, instruction_context, 1)?;
                instruction_context.check_number_of_instruction_accounts(4)?;
                let _current_authority =
                    keyed_account_at_index(keyed_accounts, first_instruction_account + 2)?;
                let authorized_pubkey =
                    &keyed_account_at_index(keyed_accounts, first_instruction_account + 3)?
                        .signer_key()
                        .ok_or(InstructionError::MissingRequiredSignature)?;
                let custodian =
                    keyed_account_at_index(keyed_accounts, first_instruction_account + 4)
                        .ok()
                        .map(|ka| ka.unsigned_key());

                me.authorize(
                    &signers,
                    authorized_pubkey,
                    stake_authorize,
                    true,
                    &clock,
                    custodian,
                )
            } else {
                Err(InstructionError::InvalidInstructionData)
            }
        }
        StakeInstruction::AuthorizeCheckedWithSeed(args) => {
            if invoke_context
                .feature_set
                .is_active(&feature_set::vote_stake_checked_instructions::id())
            {
                instruction_context.check_number_of_instruction_accounts(2)?;
                let authority_base =
                    keyed_account_at_index(keyed_accounts, first_instruction_account + 1)?;
                let clock =
                    get_sysvar_with_account_check::clock(invoke_context, instruction_context, 2)?;
                instruction_context.check_number_of_instruction_accounts(4)?;
                let authorized_pubkey =
                    &keyed_account_at_index(keyed_accounts, first_instruction_account + 3)?
                        .signer_key()
                        .ok_or(InstructionError::MissingRequiredSignature)?;
                let custodian =
                    keyed_account_at_index(keyed_accounts, first_instruction_account + 4)
                        .ok()
                        .map(|ka| ka.unsigned_key());

                me.authorize_with_seed(
                    authority_base,
                    &args.authority_seed,
                    &args.authority_owner,
                    authorized_pubkey,
                    args.stake_authorize,
                    true,
                    &clock,
                    custodian,
                )
            } else {
                Err(InstructionError::InvalidInstructionData)
            }
        }
        StakeInstruction::SetLockupChecked(lockup_checked) => {
            if invoke_context
                .feature_set
                .is_active(&feature_set::vote_stake_checked_instructions::id())
            {
                let custodian = if let Ok(custodian) =
                    keyed_account_at_index(keyed_accounts, first_instruction_account + 2)
                {
                    Some(
                        *custodian
                            .signer_key()
                            .ok_or(InstructionError::MissingRequiredSignature)?,
                    )
                } else {
                    None
                };

                let lockup = LockupArgs {
                    unix_timestamp: lockup_checked.unix_timestamp,
                    epoch: lockup_checked.epoch,
                    custodian,
                };
                let clock = invoke_context.get_sysvar_cache().get_clock()?;
                me.set_lockup(&lockup, &signers, &clock)
            } else {
                Err(InstructionError::InvalidInstructionData)
            }
        }
        StakeInstruction::GetMinimumDelegation => {
            let feature_set = invoke_context.feature_set.as_ref();
            if !feature_set.is_active(
                &feature_set::add_get_minimum_delegation_instruction_to_stake_program::id(),
            ) {
                return Err(InstructionError::InvalidInstructionData);
            }

            let minimum_delegation = crate::get_minimum_delegation(feature_set);
            let minimum_delegation = Vec::from(minimum_delegation.to_le_bytes());
            invoke_context
                .transaction_context
                .set_return_data(id(), minimum_delegation)
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::stake_state::{
            authorized_from, create_stake_history_from_delegations, from, stake_from, Delegation,
            Meta, Stake, StakeState,
        },
        bincode::serialize,
        solana_program_runtime::{
            invoke_context::mock_process_instruction, sysvar_cache::SysvarCache,
        },
        solana_sdk::{
            account::{self, AccountSharedData, ReadableAccount, WritableAccount},
            account_utils::StateMut,
            clock::{Epoch, UnixTimestamp},
            feature_set::FeatureSet,
            instruction::{AccountMeta, Instruction},
            pubkey::Pubkey,
            rent::Rent,
            stake::{
                config as stake_config,
                instruction::{self, LockupArgs},
                state::{Authorized, Lockup, StakeAuthorize},
            },
            system_program,
            sysvar::{self, stake_history::StakeHistory},
        },
        solana_vote_program::vote_state::{self, VoteState, VoteStateVersions},
        std::{collections::HashSet, str::FromStr},
    };

    fn create_default_account() -> AccountSharedData {
        AccountSharedData::new(0, 0, &Pubkey::new_unique())
    }

    fn create_default_stake_account() -> AccountSharedData {
        AccountSharedData::new(0, 0, &id())
    }

    fn invalid_stake_state_pubkey() -> Pubkey {
        Pubkey::from_str("BadStake11111111111111111111111111111111111").unwrap()
    }

    fn invalid_vote_state_pubkey() -> Pubkey {
        Pubkey::from_str("BadVote111111111111111111111111111111111111").unwrap()
    }

    fn spoofed_stake_state_pubkey() -> Pubkey {
        Pubkey::from_str("SpoofedStake1111111111111111111111111111111").unwrap()
    }

    fn spoofed_stake_program_id() -> Pubkey {
        Pubkey::from_str("Spoofed111111111111111111111111111111111111").unwrap()
    }

    fn process_instruction(
        instruction_data: &[u8],
        transaction_accounts: Vec<(Pubkey, AccountSharedData)>,
        instruction_accounts: Vec<AccountMeta>,
        expected_result: Result<(), InstructionError>,
    ) -> Vec<AccountSharedData> {
        process_instruction_with_sysvar_cache(
            instruction_data,
            transaction_accounts,
            instruction_accounts,
            None,
            expected_result,
        )
    }

    fn process_instruction_with_sysvar_cache(
        instruction_data: &[u8],
        transaction_accounts: Vec<(Pubkey, AccountSharedData)>,
        instruction_accounts: Vec<AccountMeta>,
        sysvar_cache_override: Option<&SysvarCache>,
        expected_result: Result<(), InstructionError>,
    ) -> Vec<AccountSharedData> {
        mock_process_instruction(
            &id(),
            Vec::new(),
            instruction_data,
            transaction_accounts,
            instruction_accounts,
            sysvar_cache_override,
            expected_result,
            super::process_instruction,
        )
    }

    fn process_instruction_as_one_arg(
        instruction: &Instruction,
        expected_result: Result<(), InstructionError>,
    ) -> Vec<AccountSharedData> {
        let mut pubkeys: HashSet<Pubkey> = instruction
            .accounts
            .iter()
            .map(|meta| meta.pubkey)
            .collect();
        pubkeys.insert(sysvar::clock::id());
        let transaction_accounts = pubkeys
            .iter()
            .map(|pubkey| {
                (
                    *pubkey,
                    if sysvar::clock::check_id(pubkey) {
                        account::create_account_shared_data_for_test(
                            &sysvar::clock::Clock::default(),
                        )
                    } else if sysvar::rewards::check_id(pubkey) {
                        account::create_account_shared_data_for_test(
                            &sysvar::rewards::Rewards::new(0.0),
                        )
                    } else if sysvar::stake_history::check_id(pubkey) {
                        account::create_account_shared_data_for_test(&StakeHistory::default())
                    } else if stake_config::check_id(pubkey) {
                        config::create_account(0, &stake_config::Config::default())
                    } else if sysvar::rent::check_id(pubkey) {
                        account::create_account_shared_data_for_test(&Rent::default())
                    } else if *pubkey == invalid_stake_state_pubkey() {
                        AccountSharedData::new(0, 0, &id())
                    } else if *pubkey == invalid_vote_state_pubkey() {
                        AccountSharedData::new(0, 0, &solana_vote_program::id())
                    } else if *pubkey == spoofed_stake_state_pubkey() {
                        AccountSharedData::new(0, 0, &spoofed_stake_program_id())
                    } else {
                        AccountSharedData::new(0, 0, &id())
                    },
                )
            })
            .collect();
        process_instruction(
            &instruction.data,
            transaction_accounts,
            instruction.accounts.clone(),
            expected_result,
        )
    }

    fn just_stake(meta: Meta, stake: u64) -> StakeState {
        StakeState::Stake(
            meta,
            Stake {
                delegation: Delegation {
                    stake,
                    ..Delegation::default()
                },
                ..Stake::default()
            },
        )
    }

    #[test]
    fn test_stake_process_instruction() {
        process_instruction_as_one_arg(
            &instruction::initialize(
                &Pubkey::new_unique(),
                &Authorized::default(),
                &Lockup::default(),
            ),
            Err(InstructionError::InvalidAccountData),
        );
        process_instruction_as_one_arg(
            &instruction::authorize(
                &Pubkey::new_unique(),
                &Pubkey::new_unique(),
                &Pubkey::new_unique(),
                StakeAuthorize::Staker,
                None,
            ),
            Err(InstructionError::InvalidAccountData),
        );
        process_instruction_as_one_arg(
            &instruction::split(
                &Pubkey::new_unique(),
                &Pubkey::new_unique(),
                100,
                &invalid_stake_state_pubkey(),
            )[2],
            Err(InstructionError::InvalidAccountData),
        );
        process_instruction_as_one_arg(
            &instruction::merge(
                &Pubkey::new_unique(),
                &invalid_stake_state_pubkey(),
                &Pubkey::new_unique(),
            )[0],
            Err(InstructionError::InvalidAccountData),
        );
        process_instruction_as_one_arg(
            &instruction::split_with_seed(
                &Pubkey::new_unique(),
                &Pubkey::new_unique(),
                100,
                &invalid_stake_state_pubkey(),
                &Pubkey::new_unique(),
                "seed",
            )[1],
            Err(InstructionError::InvalidAccountData),
        );
        process_instruction_as_one_arg(
            &instruction::delegate_stake(
                &Pubkey::new_unique(),
                &Pubkey::new_unique(),
                &invalid_vote_state_pubkey(),
            ),
            Err(InstructionError::InvalidAccountData),
        );
        process_instruction_as_one_arg(
            &instruction::withdraw(
                &Pubkey::new_unique(),
                &Pubkey::new_unique(),
                &Pubkey::new_unique(),
                100,
                None,
            ),
            Err(InstructionError::InvalidAccountData),
        );
        process_instruction_as_one_arg(
            &instruction::deactivate_stake(&Pubkey::new_unique(), &Pubkey::new_unique()),
            Err(InstructionError::InvalidAccountData),
        );
        process_instruction_as_one_arg(
            &instruction::set_lockup(
                &Pubkey::new_unique(),
                &LockupArgs::default(),
                &Pubkey::new_unique(),
            ),
            Err(InstructionError::InvalidAccountData),
        );
    }

    #[test]
    fn test_spoofed_stake_accounts() {
        process_instruction_as_one_arg(
            &instruction::initialize(
                &spoofed_stake_state_pubkey(),
                &Authorized::default(),
                &Lockup::default(),
            ),
            Err(InstructionError::InvalidAccountOwner),
        );
        process_instruction_as_one_arg(
            &instruction::authorize(
                &spoofed_stake_state_pubkey(),
                &Pubkey::new_unique(),
                &Pubkey::new_unique(),
                StakeAuthorize::Staker,
                None,
            ),
            Err(InstructionError::InvalidAccountOwner),
        );
        process_instruction_as_one_arg(
            &instruction::split(
                &spoofed_stake_state_pubkey(),
                &Pubkey::new_unique(),
                100,
                &Pubkey::new_unique(),
            )[2],
            Err(InstructionError::InvalidAccountOwner),
        );
        process_instruction_as_one_arg(
            &instruction::split(
                &Pubkey::new_unique(),
                &Pubkey::new_unique(),
                100,
                &spoofed_stake_state_pubkey(),
            )[2],
            Err(InstructionError::IncorrectProgramId),
        );
        process_instruction_as_one_arg(
            &instruction::merge(
                &spoofed_stake_state_pubkey(),
                &Pubkey::new_unique(),
                &Pubkey::new_unique(),
            )[0],
            Err(InstructionError::InvalidAccountOwner),
        );
        process_instruction_as_one_arg(
            &instruction::merge(
                &Pubkey::new_unique(),
                &spoofed_stake_state_pubkey(),
                &Pubkey::new_unique(),
            )[0],
            Err(InstructionError::IncorrectProgramId),
        );
        process_instruction_as_one_arg(
            &instruction::split_with_seed(
                &spoofed_stake_state_pubkey(),
                &Pubkey::new_unique(),
                100,
                &Pubkey::new_unique(),
                &Pubkey::new_unique(),
                "seed",
            )[1],
            Err(InstructionError::InvalidAccountOwner),
        );
        process_instruction_as_one_arg(
            &instruction::delegate_stake(
                &spoofed_stake_state_pubkey(),
                &Pubkey::new_unique(),
                &Pubkey::new_unique(),
            ),
            Err(InstructionError::InvalidAccountOwner),
        );
        process_instruction_as_one_arg(
            &instruction::withdraw(
                &spoofed_stake_state_pubkey(),
                &Pubkey::new_unique(),
                &Pubkey::new_unique(),
                100,
                None,
            ),
            Err(InstructionError::InvalidAccountOwner),
        );
        process_instruction_as_one_arg(
            &instruction::deactivate_stake(&spoofed_stake_state_pubkey(), &Pubkey::new_unique()),
            Err(InstructionError::InvalidAccountOwner),
        );
        process_instruction_as_one_arg(
            &instruction::set_lockup(
                &spoofed_stake_state_pubkey(),
                &LockupArgs::default(),
                &Pubkey::new_unique(),
            ),
            Err(InstructionError::InvalidAccountOwner),
        );
    }

    #[test]
    fn test_stake_process_instruction_decode_bail() {
        // these will not call stake_state, have bogus contents
        let stake_address = Pubkey::new_unique();
        let stake_account = create_default_stake_account();
        let rent_address = sysvar::rent::id();
        let rent = Rent::default();
        let rent_account = account::create_account_shared_data_for_test(&rent);
        let rewards_address = sysvar::rewards::id();
        let rewards_account =
            account::create_account_shared_data_for_test(&sysvar::rewards::Rewards::new(0.0));
        let stake_history_address = sysvar::stake_history::id();
        let stake_history_account =
            account::create_account_shared_data_for_test(&StakeHistory::default());
        let vote_address = Pubkey::new_unique();
        let vote_account = AccountSharedData::new(0, 0, &solana_vote_program::id());
        let clock_address = sysvar::clock::id();
        let clock_account =
            account::create_account_shared_data_for_test(&sysvar::clock::Clock::default());
        let config_address = stake_config::id();
        let config_account = config::create_account(0, &stake_config::Config::default());
        let rent_exempt_reserve = rent.minimum_balance(std::mem::size_of::<StakeState>());
        let minimum_delegation = crate::get_minimum_delegation(&FeatureSet::all_enabled());
        let withdrawal_amount = rent_exempt_reserve + minimum_delegation;

        // gets the "is_empty()" check
        process_instruction(
            &serialize(&StakeInstruction::Initialize(
                Authorized::default(),
                Lockup::default(),
            ))
            .unwrap(),
            Vec::new(),
            Vec::new(),
            Err(InstructionError::NotEnoughAccountKeys),
        );

        // no account for rent
        process_instruction(
            &serialize(&StakeInstruction::Initialize(
                Authorized::default(),
                Lockup::default(),
            ))
            .unwrap(),
            vec![(stake_address, stake_account.clone())],
            vec![AccountMeta {
                pubkey: stake_address,
                is_signer: false,
                is_writable: false,
            }],
            Err(InstructionError::NotEnoughAccountKeys),
        );

        // fails to deserialize stake state
        process_instruction(
            &serialize(&StakeInstruction::Initialize(
                Authorized::default(),
                Lockup::default(),
            ))
            .unwrap(),
            vec![
                (stake_address, stake_account.clone()),
                (rent_address, rent_account),
            ],
            vec![
                AccountMeta {
                    pubkey: stake_address,
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: rent_address,
                    is_signer: false,
                    is_writable: false,
                },
            ],
            Err(InstructionError::InvalidAccountData),
        );

        // gets the first check in delegate, wrong number of accounts
        process_instruction(
            &serialize(&StakeInstruction::DelegateStake).unwrap(),
            vec![(stake_address, stake_account.clone())],
            vec![AccountMeta {
                pubkey: stake_address,
                is_signer: false,
                is_writable: false,
            }],
            Err(InstructionError::NotEnoughAccountKeys),
        );

        // gets the sub-check for number of args
        process_instruction(
            &serialize(&StakeInstruction::DelegateStake).unwrap(),
            vec![(stake_address, stake_account.clone())],
            vec![AccountMeta {
                pubkey: stake_address,
                is_signer: false,
                is_writable: false,
            }],
            Err(InstructionError::NotEnoughAccountKeys),
        );

        // gets the check non-deserialize-able account in delegate_stake
        process_instruction(
            &serialize(&StakeInstruction::DelegateStake).unwrap(),
            vec![
                (stake_address, stake_account.clone()),
                (vote_address, vote_account.clone()),
                (clock_address, clock_account),
                (stake_history_address, stake_history_account.clone()),
                (config_address, config_account),
            ],
            vec![
                AccountMeta {
                    pubkey: stake_address,
                    is_signer: true,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: vote_address,
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: clock_address,
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: stake_history_address,
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: config_address,
                    is_signer: false,
                    is_writable: false,
                },
            ],
            Err(InstructionError::InvalidAccountData),
        );

        // Tests 3rd keyed account is of correct type (Clock instead of rewards) in withdraw
        process_instruction(
            &serialize(&StakeInstruction::Withdraw(withdrawal_amount)).unwrap(),
            vec![
                (stake_address, stake_account.clone()),
                (vote_address, vote_account),
                (rewards_address, rewards_account.clone()),
                (stake_history_address, stake_history_account),
            ],
            vec![
                AccountMeta {
                    pubkey: stake_address,
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: vote_address,
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: rewards_address,
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: stake_history_address,
                    is_signer: false,
                    is_writable: false,
                },
            ],
            Err(InstructionError::InvalidArgument),
        );

        // Tests correct number of accounts are provided in withdraw
        process_instruction(
            &serialize(&StakeInstruction::Withdraw(withdrawal_amount)).unwrap(),
            vec![(stake_address, stake_account.clone())],
            vec![AccountMeta {
                pubkey: stake_address,
                is_signer: false,
                is_writable: false,
            }],
            Err(InstructionError::NotEnoughAccountKeys),
        );

        // Tests 2nd keyed account is of correct type (Clock instead of rewards) in deactivate
        process_instruction(
            &serialize(&StakeInstruction::Deactivate).unwrap(),
            vec![
                (stake_address, stake_account),
                (rewards_address, rewards_account),
            ],
            vec![
                AccountMeta {
                    pubkey: stake_address,
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: rewards_address,
                    is_signer: false,
                    is_writable: false,
                },
            ],
            Err(InstructionError::InvalidArgument),
        );

        // Tests correct number of accounts are provided in deactivate
        process_instruction(
            &serialize(&StakeInstruction::Deactivate).unwrap(),
            Vec::new(),
            Vec::new(),
            Err(InstructionError::NotEnoughAccountKeys),
        );
    }

    #[test]
    fn test_stake_checked_instructions() {
        let stake_address = Pubkey::new_unique();
        let staker = Pubkey::new_unique();
        let staker_account = create_default_account();
        let withdrawer = Pubkey::new_unique();
        let withdrawer_account = create_default_account();
        let authorized_address = Pubkey::new_unique();
        let authorized_account = create_default_account();
        let new_authorized_account = create_default_account();
        let clock_address = sysvar::clock::id();
        let clock_account = account::create_account_shared_data_for_test(&Clock::default());
        let custodian = Pubkey::new_unique();
        let custodian_account = create_default_account();
        let rent = Rent::default();
        let rent_address = sysvar::rent::id();
        let rent_account = account::create_account_shared_data_for_test(&rent);
        let rent_exempt_reserve = rent.minimum_balance(std::mem::size_of::<StakeState>());
        let minimum_delegation = crate::get_minimum_delegation(&FeatureSet::all_enabled());

        // Test InitializeChecked with non-signing withdrawer
        let mut instruction =
            initialize_checked(&stake_address, &Authorized { staker, withdrawer });
        instruction.accounts[3] = AccountMeta::new_readonly(withdrawer, false);
        process_instruction_as_one_arg(
            &instruction,
            Err(InstructionError::MissingRequiredSignature),
        );

        // Test InitializeChecked with withdrawer signer
        let stake_account = AccountSharedData::new(
            rent_exempt_reserve + minimum_delegation,
            std::mem::size_of::<crate::stake_state::StakeState>(),
            &id(),
        );
        process_instruction(
            &serialize(&StakeInstruction::InitializeChecked).unwrap(),
            vec![
                (stake_address, stake_account),
                (rent_address, rent_account),
                (staker, staker_account),
                (withdrawer, withdrawer_account.clone()),
            ],
            vec![
                AccountMeta {
                    pubkey: stake_address,
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: rent_address,
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: staker,
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: withdrawer,
                    is_signer: true,
                    is_writable: false,
                },
            ],
            Ok(()),
        );

        // Test AuthorizeChecked with non-signing authority
        let mut instruction = authorize_checked(
            &stake_address,
            &authorized_address,
            &staker,
            StakeAuthorize::Staker,
            None,
        );
        instruction.accounts[3] = AccountMeta::new_readonly(staker, false);
        process_instruction_as_one_arg(
            &instruction,
            Err(InstructionError::MissingRequiredSignature),
        );

        let mut instruction = authorize_checked(
            &stake_address,
            &authorized_address,
            &withdrawer,
            StakeAuthorize::Withdrawer,
            None,
        );
        instruction.accounts[3] = AccountMeta::new_readonly(withdrawer, false);
        process_instruction_as_one_arg(
            &instruction,
            Err(InstructionError::MissingRequiredSignature),
        );

        // Test AuthorizeChecked with authority signer
        let stake_account = AccountSharedData::new_data_with_space(
            42,
            &StakeState::Initialized(Meta::auto(&authorized_address)),
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .unwrap();
        process_instruction(
            &serialize(&StakeInstruction::AuthorizeChecked(StakeAuthorize::Staker)).unwrap(),
            vec![
                (stake_address, stake_account.clone()),
                (clock_address, clock_account.clone()),
                (authorized_address, authorized_account.clone()),
                (staker, new_authorized_account.clone()),
            ],
            vec![
                AccountMeta {
                    pubkey: stake_address,
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: clock_address,
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: authorized_address,
                    is_signer: true,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: staker,
                    is_signer: true,
                    is_writable: false,
                },
            ],
            Ok(()),
        );

        process_instruction(
            &serialize(&StakeInstruction::AuthorizeChecked(
                StakeAuthorize::Withdrawer,
            ))
            .unwrap(),
            vec![
                (stake_address, stake_account),
                (clock_address, clock_account.clone()),
                (authorized_address, authorized_account.clone()),
                (withdrawer, new_authorized_account.clone()),
            ],
            vec![
                AccountMeta {
                    pubkey: stake_address,
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: clock_address,
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: authorized_address,
                    is_signer: true,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: withdrawer,
                    is_signer: true,
                    is_writable: false,
                },
            ],
            Ok(()),
        );

        // Test AuthorizeCheckedWithSeed with non-signing authority
        let authorized_owner = Pubkey::new_unique();
        let seed = "test seed";
        let address_with_seed =
            Pubkey::create_with_seed(&authorized_owner, seed, &authorized_owner).unwrap();
        let mut instruction = authorize_checked_with_seed(
            &stake_address,
            &authorized_owner,
            seed.to_string(),
            &authorized_owner,
            &staker,
            StakeAuthorize::Staker,
            None,
        );
        instruction.accounts[3] = AccountMeta::new_readonly(staker, false);
        process_instruction_as_one_arg(
            &instruction,
            Err(InstructionError::MissingRequiredSignature),
        );

        let mut instruction = authorize_checked_with_seed(
            &stake_address,
            &authorized_owner,
            seed.to_string(),
            &authorized_owner,
            &staker,
            StakeAuthorize::Withdrawer,
            None,
        );
        instruction.accounts[3] = AccountMeta::new_readonly(staker, false);
        process_instruction_as_one_arg(
            &instruction,
            Err(InstructionError::MissingRequiredSignature),
        );

        // Test AuthorizeCheckedWithSeed with authority signer
        let stake_account = AccountSharedData::new_data_with_space(
            42,
            &StakeState::Initialized(Meta::auto(&address_with_seed)),
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .unwrap();
        process_instruction(
            &serialize(&StakeInstruction::AuthorizeCheckedWithSeed(
                AuthorizeCheckedWithSeedArgs {
                    stake_authorize: StakeAuthorize::Staker,
                    authority_seed: seed.to_string(),
                    authority_owner: authorized_owner,
                },
            ))
            .unwrap(),
            vec![
                (address_with_seed, stake_account.clone()),
                (authorized_owner, authorized_account.clone()),
                (clock_address, clock_account.clone()),
                (staker, new_authorized_account.clone()),
            ],
            vec![
                AccountMeta {
                    pubkey: address_with_seed,
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: authorized_owner,
                    is_signer: true,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: clock_address,
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: staker,
                    is_signer: true,
                    is_writable: false,
                },
            ],
            Ok(()),
        );

        process_instruction(
            &serialize(&StakeInstruction::AuthorizeCheckedWithSeed(
                AuthorizeCheckedWithSeedArgs {
                    stake_authorize: StakeAuthorize::Withdrawer,
                    authority_seed: seed.to_string(),
                    authority_owner: authorized_owner,
                },
            ))
            .unwrap(),
            vec![
                (address_with_seed, stake_account),
                (authorized_owner, authorized_account),
                (clock_address, clock_account.clone()),
                (withdrawer, new_authorized_account),
            ],
            vec![
                AccountMeta {
                    pubkey: address_with_seed,
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: authorized_owner,
                    is_signer: true,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: clock_address,
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: withdrawer,
                    is_signer: true,
                    is_writable: false,
                },
            ],
            Ok(()),
        );

        // Test SetLockupChecked with non-signing lockup custodian
        let mut instruction = set_lockup_checked(
            &stake_address,
            &LockupArgs {
                unix_timestamp: None,
                epoch: Some(1),
                custodian: Some(custodian),
            },
            &withdrawer,
        );
        instruction.accounts[2] = AccountMeta::new_readonly(custodian, false);
        process_instruction_as_one_arg(
            &instruction,
            Err(InstructionError::MissingRequiredSignature),
        );

        // Test SetLockupChecked with lockup custodian signer
        let stake_account = AccountSharedData::new_data_with_space(
            42,
            &StakeState::Initialized(Meta::auto(&withdrawer)),
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .unwrap();

        process_instruction(
            &instruction.data,
            vec![
                (clock_address, clock_account),
                (stake_address, stake_account),
                (withdrawer, withdrawer_account),
                (custodian, custodian_account),
            ],
            vec![
                AccountMeta {
                    pubkey: stake_address,
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: withdrawer,
                    is_signer: true,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: custodian,
                    is_signer: true,
                    is_writable: false,
                },
            ],
            Ok(()),
        );
    }

    #[test]
    fn test_stake_initialize() {
        let rent = Rent::default();
        let rent_exempt_reserve = rent.minimum_balance(std::mem::size_of::<StakeState>());
        let minimum_delegation = crate::get_minimum_delegation(&FeatureSet::all_enabled());
        let stake_lamports = rent_exempt_reserve + minimum_delegation;
        let stake_address = solana_sdk::pubkey::new_rand();
        let stake_account =
            AccountSharedData::new(stake_lamports, std::mem::size_of::<StakeState>(), &id());
        let custodian_address = solana_sdk::pubkey::new_rand();
        let lockup = Lockup {
            epoch: 1,
            unix_timestamp: 0,
            custodian: custodian_address,
        };
        let instruction_data = serialize(&StakeInstruction::Initialize(
            Authorized::auto(&stake_address),
            lockup,
        ))
        .unwrap();
        let mut transaction_accounts = vec![
            (stake_address, stake_account.clone()),
            (
                sysvar::rent::id(),
                account::create_account_shared_data_for_test(&rent),
            ),
        ];
        let instruction_accounts = vec![
            AccountMeta {
                pubkey: stake_address,
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: sysvar::rent::id(),
                is_signer: false,
                is_writable: false,
            },
        ];

        // should pass
        let accounts = process_instruction(
            &instruction_data,
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Ok(()),
        );
        // check that we see what we expect
        assert_eq!(
            from(&accounts[0]).unwrap(),
            StakeState::Initialized(Meta {
                authorized: Authorized::auto(&stake_address),
                rent_exempt_reserve,
                lockup,
            }),
        );

        // 2nd time fails, can't move it from anything other than uninit->init
        transaction_accounts[0] = (stake_address, accounts[0].clone());
        process_instruction(
            &instruction_data,
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Err(InstructionError::InvalidAccountData),
        );
        transaction_accounts[0] = (stake_address, stake_account);

        // not enough balance for rent and minimum delegation...
        transaction_accounts[1] = (
            sysvar::rent::id(),
            account::create_account_shared_data_for_test(&Rent {
                lamports_per_byte_year: rent.lamports_per_byte_year + 1,
                ..rent
            }),
        );
        process_instruction(
            &instruction_data,
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Err(InstructionError::InsufficientFunds),
        );

        // incorrect account sizes
        let stake_account =
            AccountSharedData::new(stake_lamports, std::mem::size_of::<StakeState>() + 1, &id());
        transaction_accounts[0] = (stake_address, stake_account);
        process_instruction(
            &instruction_data,
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Err(InstructionError::InvalidAccountData),
        );

        let stake_account =
            AccountSharedData::new(stake_lamports, std::mem::size_of::<StakeState>() - 1, &id());
        transaction_accounts[0] = (stake_address, stake_account);
        process_instruction(
            &instruction_data,
            transaction_accounts,
            instruction_accounts,
            Err(InstructionError::InvalidAccountData),
        );
    }

    #[test]
    fn test_authorize() {
        let authority_address = solana_sdk::pubkey::new_rand();
        let authority_address_2 = solana_sdk::pubkey::new_rand();
        let stake_address = solana_sdk::pubkey::new_rand();
        let stake_lamports = 42;
        let stake_account = AccountSharedData::new_data_with_space(
            stake_lamports,
            &StakeState::default(),
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .unwrap();
        let to_address = solana_sdk::pubkey::new_rand();
        let to_account = AccountSharedData::new(1, 0, &system_program::id());
        let mut transaction_accounts = vec![
            (stake_address, stake_account),
            (to_address, to_account),
            (authority_address, AccountSharedData::default()),
            (
                sysvar::clock::id(),
                account::create_account_shared_data_for_test(&Clock::default()),
            ),
            (
                sysvar::stake_history::id(),
                account::create_account_shared_data_for_test(&StakeHistory::default()),
            ),
        ];
        let mut instruction_accounts = vec![
            AccountMeta {
                pubkey: stake_address,
                is_signer: true,
                is_writable: false,
            },
            AccountMeta {
                pubkey: sysvar::clock::id(),
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: authority_address,
                is_signer: false,
                is_writable: false,
            },
        ];

        // should fail, uninit
        process_instruction(
            &serialize(&StakeInstruction::Authorize(
                authority_address,
                StakeAuthorize::Staker,
            ))
            .unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Err(InstructionError::InvalidAccountData),
        );

        // should pass
        let stake_account = AccountSharedData::new_data_with_space(
            stake_lamports,
            &StakeState::Initialized(Meta::auto(&stake_address)),
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .unwrap();
        transaction_accounts[0] = (stake_address, stake_account);
        let accounts = process_instruction(
            &serialize(&StakeInstruction::Authorize(
                authority_address,
                StakeAuthorize::Staker,
            ))
            .unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Ok(()),
        );
        transaction_accounts[0] = (stake_address, accounts[0].clone());
        let accounts = process_instruction(
            &serialize(&StakeInstruction::Authorize(
                authority_address,
                StakeAuthorize::Withdrawer,
            ))
            .unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Ok(()),
        );
        transaction_accounts[0] = (stake_address, accounts[0].clone());
        if let StakeState::Initialized(Meta { authorized, .. }) = from(&accounts[0]).unwrap() {
            assert_eq!(authorized.staker, authority_address);
            assert_eq!(authorized.withdrawer, authority_address);
        } else {
            panic!();
        }

        // A second authorization signed by the stake account should fail
        process_instruction(
            &serialize(&StakeInstruction::Authorize(
                authority_address_2,
                StakeAuthorize::Staker,
            ))
            .unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Err(InstructionError::MissingRequiredSignature),
        );

        // Test a second authorization by the new authority_address
        instruction_accounts[0].is_signer = false;
        instruction_accounts[2].is_signer = true;
        let accounts = process_instruction(
            &serialize(&StakeInstruction::Authorize(
                authority_address_2,
                StakeAuthorize::Staker,
            ))
            .unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Ok(()),
        );
        if let StakeState::Initialized(Meta { authorized, .. }) = from(&accounts[0]).unwrap() {
            assert_eq!(authorized.staker, authority_address_2);
        } else {
            panic!();
        }

        // Test a successful action by the currently authorized withdrawer
        let mut instruction_accounts = vec![
            AccountMeta {
                pubkey: stake_address,
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: to_address,
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: sysvar::clock::id(),
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: sysvar::stake_history::id(),
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: authority_address,
                is_signer: true,
                is_writable: false,
            },
        ];
        let accounts = process_instruction(
            &serialize(&StakeInstruction::Withdraw(stake_lamports)).unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Ok(()),
        );
        assert_eq!(from(&accounts[0]).unwrap(), StakeState::Uninitialized);

        // Test that withdrawal to account fails without authorized withdrawer
        instruction_accounts[4].is_signer = false;
        process_instruction(
            &serialize(&StakeInstruction::Withdraw(stake_lamports)).unwrap(),
            transaction_accounts,
            instruction_accounts,
            Err(InstructionError::MissingRequiredSignature),
        );
    }

    #[test]
    fn test_authorize_override() {
        let authority_address = solana_sdk::pubkey::new_rand();
        let mallory_address = solana_sdk::pubkey::new_rand();
        let stake_address = solana_sdk::pubkey::new_rand();
        let stake_lamports = 42;
        let stake_account = AccountSharedData::new_data_with_space(
            stake_lamports,
            &StakeState::Initialized(Meta::auto(&stake_address)),
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .unwrap();
        let mut transaction_accounts = vec![
            (stake_address, stake_account),
            (authority_address, AccountSharedData::default()),
            (
                sysvar::clock::id(),
                account::create_account_shared_data_for_test(&Clock::default()),
            ),
        ];
        let mut instruction_accounts = vec![
            AccountMeta {
                pubkey: stake_address,
                is_signer: true,
                is_writable: false,
            },
            AccountMeta {
                pubkey: sysvar::clock::id(),
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: authority_address,
                is_signer: false,
                is_writable: false,
            },
        ];

        // Authorize a staker pubkey and move the withdrawer key into cold storage.
        let accounts = process_instruction(
            &serialize(&StakeInstruction::Authorize(
                authority_address,
                StakeAuthorize::Staker,
            ))
            .unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Ok(()),
        );
        transaction_accounts[0] = (stake_address, accounts[0].clone());

        // Attack! The stake key (a hot key) is stolen and used to authorize a new staker.
        instruction_accounts[0].is_signer = false;
        instruction_accounts[2].is_signer = true;
        let accounts = process_instruction(
            &serialize(&StakeInstruction::Authorize(
                mallory_address,
                StakeAuthorize::Staker,
            ))
            .unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Ok(()),
        );
        transaction_accounts[0] = (stake_address, accounts[0].clone());

        // Verify the original staker no longer has access.
        process_instruction(
            &serialize(&StakeInstruction::Authorize(
                authority_address,
                StakeAuthorize::Staker,
            ))
            .unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Err(InstructionError::MissingRequiredSignature),
        );

        // Verify the withdrawer (pulled from cold storage) can save the day.
        instruction_accounts[0].is_signer = true;
        instruction_accounts[2].is_signer = false;
        let accounts = process_instruction(
            &serialize(&StakeInstruction::Authorize(
                authority_address,
                StakeAuthorize::Withdrawer,
            ))
            .unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Ok(()),
        );
        transaction_accounts[0] = (stake_address, accounts[0].clone());

        // Attack! Verify the staker cannot be used to authorize a withdraw.
        instruction_accounts[0].is_signer = false;
        instruction_accounts[2] = AccountMeta {
            pubkey: mallory_address,
            is_signer: true,
            is_writable: false,
        };
        process_instruction(
            &serialize(&StakeInstruction::Authorize(
                authority_address,
                StakeAuthorize::Withdrawer,
            ))
            .unwrap(),
            transaction_accounts,
            instruction_accounts,
            Err(InstructionError::MissingRequiredSignature),
        );
    }

    #[test]
    fn test_authorize_with_seed() {
        let authority_base_address = solana_sdk::pubkey::new_rand();
        let authority_address = solana_sdk::pubkey::new_rand();
        let seed = "42";
        let stake_address = Pubkey::create_with_seed(&authority_base_address, seed, &id()).unwrap();
        let stake_lamports = 42;
        let stake_account = AccountSharedData::new_data_with_space(
            stake_lamports,
            &StakeState::Initialized(Meta::auto(&stake_address)),
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .unwrap();
        let mut transaction_accounts = vec![
            (stake_address, stake_account),
            (authority_base_address, AccountSharedData::default()),
            (
                sysvar::clock::id(),
                account::create_account_shared_data_for_test(&Clock::default()),
            ),
        ];
        let mut instruction_accounts = vec![
            AccountMeta {
                pubkey: stake_address,
                is_signer: true,
                is_writable: false,
            },
            AccountMeta {
                pubkey: authority_base_address,
                is_signer: true,
                is_writable: false,
            },
            AccountMeta {
                pubkey: sysvar::clock::id(),
                is_signer: false,
                is_writable: false,
            },
        ];

        // Wrong seed
        process_instruction(
            &serialize(&StakeInstruction::AuthorizeWithSeed(
                AuthorizeWithSeedArgs {
                    new_authorized_pubkey: authority_address,
                    stake_authorize: StakeAuthorize::Staker,
                    authority_seed: "".to_string(),
                    authority_owner: id(),
                },
            ))
            .unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Err(InstructionError::MissingRequiredSignature),
        );

        // Wrong base
        instruction_accounts[1].pubkey = authority_address;
        let instruction_data = serialize(&StakeInstruction::AuthorizeWithSeed(
            AuthorizeWithSeedArgs {
                new_authorized_pubkey: authority_address,
                stake_authorize: StakeAuthorize::Staker,
                authority_seed: seed.to_string(),
                authority_owner: id(),
            },
        ))
        .unwrap();
        process_instruction(
            &instruction_data,
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Err(InstructionError::MissingRequiredSignature),
        );
        instruction_accounts[1].pubkey = authority_base_address;

        // Set stake authority
        let accounts = process_instruction(
            &instruction_data,
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Ok(()),
        );
        transaction_accounts[0] = (stake_address, accounts[0].clone());

        // Set withdraw authority
        let instruction_data = serialize(&StakeInstruction::AuthorizeWithSeed(
            AuthorizeWithSeedArgs {
                new_authorized_pubkey: authority_address,
                stake_authorize: StakeAuthorize::Withdrawer,
                authority_seed: seed.to_string(),
                authority_owner: id(),
            },
        ))
        .unwrap();
        let accounts = process_instruction(
            &instruction_data,
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Ok(()),
        );
        transaction_accounts[0] = (stake_address, accounts[0].clone());

        // No longer withdraw authority
        process_instruction(
            &instruction_data,
            transaction_accounts,
            instruction_accounts,
            Err(InstructionError::MissingRequiredSignature),
        );
    }

    #[test]
    fn test_authorize_delegated_stake() {
        let authority_address = solana_sdk::pubkey::new_rand();
        let stake_address = solana_sdk::pubkey::new_rand();
        let stake_lamports = 42;
        let stake_account = AccountSharedData::new_data_with_space(
            stake_lamports,
            &StakeState::Initialized(Meta::auto(&stake_address)),
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .unwrap();
        let vote_address = solana_sdk::pubkey::new_rand();
        let vote_account =
            vote_state::create_account(&vote_address, &solana_sdk::pubkey::new_rand(), 0, 100);
        let vote_address_2 = solana_sdk::pubkey::new_rand();
        let mut vote_account_2 =
            vote_state::create_account(&vote_address_2, &solana_sdk::pubkey::new_rand(), 0, 100);
        vote_account_2.set_state(&VoteState::default()).unwrap();
        let mut transaction_accounts = vec![
            (stake_address, stake_account),
            (vote_address, vote_account),
            (vote_address_2, vote_account_2),
            (
                authority_address,
                AccountSharedData::new(42, 0, &system_program::id()),
            ),
            (
                sysvar::clock::id(),
                account::create_account_shared_data_for_test(&Clock::default()),
            ),
            (
                sysvar::stake_history::id(),
                account::create_account_shared_data_for_test(&StakeHistory::default()),
            ),
            (
                stake_config::id(),
                config::create_account(0, &stake_config::Config::default()),
            ),
        ];
        let mut instruction_accounts = vec![
            AccountMeta {
                pubkey: stake_address,
                is_signer: true,
                is_writable: false,
            },
            AccountMeta {
                pubkey: vote_address,
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: sysvar::clock::id(),
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: sysvar::stake_history::id(),
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: stake_config::id(),
                is_signer: false,
                is_writable: false,
            },
        ];

        // delegate stake
        let accounts = process_instruction(
            &serialize(&StakeInstruction::DelegateStake).unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Ok(()),
        );
        transaction_accounts[0] = (stake_address, accounts[0].clone());

        // deactivate, so we can re-delegate
        let accounts = process_instruction(
            &serialize(&StakeInstruction::Deactivate).unwrap(),
            transaction_accounts.clone(),
            vec![
                AccountMeta {
                    pubkey: stake_address,
                    is_signer: true,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: sysvar::clock::id(),
                    is_signer: false,
                    is_writable: false,
                },
            ],
            Ok(()),
        );
        transaction_accounts[0] = (stake_address, accounts[0].clone());

        // authorize
        let accounts = process_instruction(
            &serialize(&StakeInstruction::Authorize(
                authority_address,
                StakeAuthorize::Staker,
            ))
            .unwrap(),
            transaction_accounts.clone(),
            vec![
                AccountMeta {
                    pubkey: stake_address,
                    is_signer: true,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: sysvar::clock::id(),
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: authority_address,
                    is_signer: false,
                    is_writable: false,
                },
            ],
            Ok(()),
        );
        transaction_accounts[0] = (stake_address, accounts[0].clone());
        assert_eq!(
            authorized_from(&accounts[0]).unwrap().staker,
            authority_address
        );

        // Random other account should fail
        instruction_accounts[0].is_signer = false;
        instruction_accounts[1].pubkey = vote_address_2;
        process_instruction(
            &serialize(&StakeInstruction::DelegateStake).unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Err(InstructionError::MissingRequiredSignature),
        );

        // Authorized staker should succeed
        instruction_accounts.push(AccountMeta {
            pubkey: authority_address,
            is_signer: true,
            is_writable: false,
        });
        let accounts = process_instruction(
            &serialize(&StakeInstruction::DelegateStake).unwrap(),
            transaction_accounts.clone(),
            instruction_accounts,
            Ok(()),
        );
        transaction_accounts[0] = (stake_address, accounts[0].clone());
        assert_eq!(
            stake_from(&accounts[0]).unwrap().delegation.voter_pubkey,
            vote_address_2,
        );

        // Test another staking action
        process_instruction(
            &serialize(&StakeInstruction::Deactivate).unwrap(),
            transaction_accounts,
            vec![
                AccountMeta {
                    pubkey: stake_address,
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: sysvar::clock::id(),
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: authority_address,
                    is_signer: true,
                    is_writable: false,
                },
            ],
            Ok(()),
        );
    }

    #[test]
    fn test_stake_delegate() {
        let mut vote_state = VoteState::default();
        for i in 0..1000 {
            vote_state.process_slot_vote_unchecked(i);
        }
        let vote_state_credits = vote_state.credits();
        let vote_address = solana_sdk::pubkey::new_rand();
        let vote_address_2 = solana_sdk::pubkey::new_rand();
        let mut vote_account =
            vote_state::create_account(&vote_address, &solana_sdk::pubkey::new_rand(), 0, 100);
        let mut vote_account_2 =
            vote_state::create_account(&vote_address_2, &solana_sdk::pubkey::new_rand(), 0, 100);
        vote_account
            .set_state(&VoteStateVersions::new_current(vote_state.clone()))
            .unwrap();
        vote_account_2
            .set_state(&VoteStateVersions::new_current(vote_state))
            .unwrap();
        let stake_lamports = 42;
        let stake_address = solana_sdk::pubkey::new_rand();
        let mut stake_account = AccountSharedData::new_data_with_space(
            stake_lamports,
            &StakeState::Initialized(Meta {
                authorized: Authorized {
                    staker: stake_address,
                    withdrawer: stake_address,
                },
                ..Meta::default()
            }),
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .unwrap();
        let mut clock = Clock {
            epoch: 1,
            ..Clock::default()
        };
        let mut transaction_accounts = vec![
            (stake_address, stake_account.clone()),
            (vote_address, vote_account),
            (vote_address_2, vote_account_2.clone()),
            (
                sysvar::clock::id(),
                account::create_account_shared_data_for_test(&clock),
            ),
            (
                sysvar::stake_history::id(),
                account::create_account_shared_data_for_test(&StakeHistory::default()),
            ),
            (
                stake_config::id(),
                config::create_account(0, &stake_config::Config::default()),
            ),
        ];
        let mut instruction_accounts = vec![
            AccountMeta {
                pubkey: stake_address,
                is_signer: true,
                is_writable: false,
            },
            AccountMeta {
                pubkey: vote_address,
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: sysvar::clock::id(),
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: sysvar::stake_history::id(),
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: stake_config::id(),
                is_signer: false,
                is_writable: false,
            },
        ];

        // should fail, unsigned stake account
        instruction_accounts[0].is_signer = false;
        process_instruction(
            &serialize(&StakeInstruction::DelegateStake).unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Err(InstructionError::MissingRequiredSignature),
        );
        instruction_accounts[0].is_signer = true;

        // should pass
        let accounts = process_instruction(
            &serialize(&StakeInstruction::DelegateStake).unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Ok(()),
        );
        // verify that delegate() looks right, compare against hand-rolled
        assert_eq!(
            stake_from(&accounts[0]).unwrap(),
            Stake {
                delegation: Delegation {
                    voter_pubkey: vote_address,
                    stake: stake_lamports,
                    activation_epoch: clock.epoch,
                    deactivation_epoch: std::u64::MAX,
                    ..Delegation::default()
                },
                credits_observed: vote_state_credits,
            }
        );

        // verify that delegate fails as stake is active and not deactivating
        clock.epoch += 1;
        transaction_accounts[0] = (stake_address, accounts[0].clone());
        transaction_accounts[3] = (
            sysvar::clock::id(),
            account::create_account_shared_data_for_test(&clock),
        );
        process_instruction(
            &serialize(&StakeInstruction::DelegateStake).unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Err(StakeError::TooSoonToRedelegate.into()),
        );

        // deactivate
        let accounts = process_instruction(
            &serialize(&StakeInstruction::Deactivate).unwrap(),
            transaction_accounts.clone(),
            vec![
                AccountMeta {
                    pubkey: stake_address,
                    is_signer: true,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: sysvar::clock::id(),
                    is_signer: false,
                    is_writable: false,
                },
            ],
            Ok(()),
        );

        // verify that delegate to a different vote account fails
        // during deactivation
        transaction_accounts[0] = (stake_address, accounts[0].clone());
        instruction_accounts[1].pubkey = vote_address_2;
        process_instruction(
            &serialize(&StakeInstruction::DelegateStake).unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Err(StakeError::TooSoonToRedelegate.into()),
        );
        instruction_accounts[1].pubkey = vote_address;

        // verify that delegate succeeds to same vote account
        // when stake is deactivating
        let accounts_2 = process_instruction(
            &serialize(&StakeInstruction::DelegateStake).unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Ok(()),
        );
        // verify that deactivation has been cleared
        let stake = stake_from(&accounts_2[0]).unwrap();
        assert_eq!(stake.delegation.deactivation_epoch, std::u64::MAX);

        // verify that delegate to a different vote account fails
        // if stake is still active
        transaction_accounts[0] = (stake_address, accounts_2[0].clone());
        instruction_accounts[1].pubkey = vote_address_2;
        process_instruction(
            &serialize(&StakeInstruction::DelegateStake).unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Err(StakeError::TooSoonToRedelegate.into()),
        );

        // without stake history, cool down is instantaneous
        clock.epoch += 1;
        transaction_accounts[3] = (
            sysvar::clock::id(),
            account::create_account_shared_data_for_test(&clock),
        );

        // verify that delegate can be called to new vote account, 2nd is redelegate
        transaction_accounts[0] = (stake_address, accounts[0].clone());
        let accounts = process_instruction(
            &serialize(&StakeInstruction::DelegateStake).unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Ok(()),
        );
        instruction_accounts[1].pubkey = vote_address;
        // verify that delegate() looks right, compare against hand-rolled
        assert_eq!(
            stake_from(&accounts[0]).unwrap(),
            Stake {
                delegation: Delegation {
                    voter_pubkey: vote_address_2,
                    stake: stake_lamports,
                    activation_epoch: clock.epoch,
                    deactivation_epoch: std::u64::MAX,
                    ..Delegation::default()
                },
                credits_observed: vote_state_credits,
            }
        );

        // signed but faked vote account
        transaction_accounts[1] = (vote_address_2, vote_account_2);
        transaction_accounts[1]
            .1
            .set_owner(solana_sdk::pubkey::new_rand());
        process_instruction(
            &serialize(&StakeInstruction::DelegateStake).unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Err(solana_sdk::instruction::InstructionError::IncorrectProgramId),
        );

        // verify that non-stakes fail delegate()
        let stake_state = StakeState::RewardsPool;
        stake_account.set_state(&stake_state).unwrap();
        transaction_accounts[0] = (stake_address, stake_account);
        process_instruction(
            &serialize(&StakeInstruction::DelegateStake).unwrap(),
            transaction_accounts,
            instruction_accounts,
            Err(solana_sdk::instruction::InstructionError::IncorrectProgramId),
        );
    }

    #[test]
    fn test_redelegate_consider_balance_changes() {
        let mut clock = Clock::default();
        let rent = Rent::default();
        let rent_exempt_reserve = rent.minimum_balance(std::mem::size_of::<StakeState>());
        let initial_lamports = 4242424242;
        let stake_lamports = rent_exempt_reserve + initial_lamports;
        let recipient_address = solana_sdk::pubkey::new_rand();
        let authority_address = solana_sdk::pubkey::new_rand();
        let vote_address = solana_sdk::pubkey::new_rand();
        let vote_account =
            vote_state::create_account(&vote_address, &solana_sdk::pubkey::new_rand(), 0, 100);
        let stake_address = solana_sdk::pubkey::new_rand();
        let stake_account = AccountSharedData::new_data_with_space(
            stake_lamports,
            &StakeState::Initialized(Meta {
                rent_exempt_reserve,
                ..Meta::auto(&authority_address)
            }),
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .unwrap();
        let mut transaction_accounts = vec![
            (stake_address, stake_account),
            (vote_address, vote_account),
            (
                recipient_address,
                AccountSharedData::new(1, 0, &system_program::id()),
            ),
            (authority_address, AccountSharedData::default()),
            (
                sysvar::clock::id(),
                account::create_account_shared_data_for_test(&clock),
            ),
            (
                sysvar::stake_history::id(),
                account::create_account_shared_data_for_test(&StakeHistory::default()),
            ),
            (
                stake_config::id(),
                config::create_account(0, &stake_config::Config::default()),
            ),
        ];
        let delegate_instruction_accounts = vec![
            AccountMeta {
                pubkey: stake_address,
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: vote_address,
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: sysvar::clock::id(),
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: sysvar::stake_history::id(),
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: stake_config::id(),
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: authority_address,
                is_signer: true,
                is_writable: false,
            },
        ];
        let deactivate_instruction_accounts = vec![
            AccountMeta {
                pubkey: stake_address,
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: sysvar::clock::id(),
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: authority_address,
                is_signer: true,
                is_writable: false,
            },
        ];

        let accounts = process_instruction(
            &serialize(&StakeInstruction::DelegateStake).unwrap(),
            transaction_accounts.clone(),
            delegate_instruction_accounts.clone(),
            Ok(()),
        );
        transaction_accounts[0] = (stake_address, accounts[0].clone());

        clock.epoch += 1;
        transaction_accounts[2] = (
            sysvar::clock::id(),
            account::create_account_shared_data_for_test(&clock),
        );
        let accounts = process_instruction(
            &serialize(&StakeInstruction::Deactivate).unwrap(),
            transaction_accounts.clone(),
            deactivate_instruction_accounts.clone(),
            Ok(()),
        );
        transaction_accounts[0] = (stake_address, accounts[0].clone());

        // Once deactivated, we withdraw stake to new account
        clock.epoch += 1;
        transaction_accounts[2] = (
            sysvar::clock::id(),
            account::create_account_shared_data_for_test(&clock),
        );
        let withdraw_lamports = initial_lamports / 2;
        let accounts = process_instruction(
            &serialize(&StakeInstruction::Withdraw(withdraw_lamports)).unwrap(),
            transaction_accounts.clone(),
            vec![
                AccountMeta {
                    pubkey: stake_address,
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: recipient_address,
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: sysvar::clock::id(),
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: sysvar::stake_history::id(),
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: authority_address,
                    is_signer: true,
                    is_writable: false,
                },
            ],
            Ok(()),
        );
        let expected_balance = rent_exempt_reserve + initial_lamports - withdraw_lamports;
        assert_eq!(accounts[0].lamports(), expected_balance);
        transaction_accounts[0] = (stake_address, accounts[0].clone());

        clock.epoch += 1;
        transaction_accounts[2] = (
            sysvar::clock::id(),
            account::create_account_shared_data_for_test(&clock),
        );
        let accounts = process_instruction(
            &serialize(&StakeInstruction::DelegateStake).unwrap(),
            transaction_accounts.clone(),
            delegate_instruction_accounts.clone(),
            Ok(()),
        );
        assert_eq!(
            stake_from(&accounts[0]).unwrap().delegation.stake,
            accounts[0].lamports() - rent_exempt_reserve,
        );
        transaction_accounts[0] = (stake_address, accounts[0].clone());

        clock.epoch += 1;
        transaction_accounts[2] = (
            sysvar::clock::id(),
            account::create_account_shared_data_for_test(&clock),
        );
        let accounts = process_instruction(
            &serialize(&StakeInstruction::Deactivate).unwrap(),
            transaction_accounts.clone(),
            deactivate_instruction_accounts,
            Ok(()),
        );
        transaction_accounts[0] = (stake_address, accounts[0].clone());

        // Out of band deposit
        transaction_accounts[0]
            .1
            .checked_add_lamports(withdraw_lamports)
            .unwrap();

        clock.epoch += 1;
        transaction_accounts[2] = (
            sysvar::clock::id(),
            account::create_account_shared_data_for_test(&clock),
        );
        let accounts = process_instruction(
            &serialize(&StakeInstruction::DelegateStake).unwrap(),
            transaction_accounts,
            delegate_instruction_accounts,
            Ok(()),
        );
        assert_eq!(
            stake_from(&accounts[0]).unwrap().delegation.stake,
            accounts[0].lamports() - rent_exempt_reserve,
        );
    }

    #[test]
    fn test_split() {
        let stake_address = solana_sdk::pubkey::new_rand();
        let minimum_delegation = crate::get_minimum_delegation(&FeatureSet::all_enabled());
        let stake_lamports = minimum_delegation * 2;
        let split_to_address = solana_sdk::pubkey::new_rand();
        let split_to_account = AccountSharedData::new_data_with_space(
            0,
            &StakeState::Uninitialized,
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .unwrap();
        let mut transaction_accounts = vec![
            (stake_address, AccountSharedData::default()),
            (split_to_address, split_to_account),
        ];
        let instruction_accounts = vec![
            AccountMeta {
                pubkey: stake_address,
                is_signer: true,
                is_writable: false,
            },
            AccountMeta {
                pubkey: split_to_address,
                is_signer: false,
                is_writable: false,
            },
        ];

        // Define rent here so that it's used consistently for setting the rent exempt reserve
        // and in the sysvar cache used for mock instruction processing.
        let mut sysvar_cache_override = SysvarCache::default();
        sysvar_cache_override.set_rent(Rent {
            lamports_per_byte_year: 0,
            ..Rent::default()
        });

        for state in [
            StakeState::Initialized(Meta::auto(&stake_address)),
            just_stake(Meta::auto(&stake_address), stake_lamports),
        ] {
            let stake_account = AccountSharedData::new_data_with_space(
                stake_lamports,
                &state,
                std::mem::size_of::<StakeState>(),
                &id(),
            )
            .unwrap();
            transaction_accounts[0] = (stake_address, stake_account);

            // should fail, split more than available
            process_instruction_with_sysvar_cache(
                &serialize(&StakeInstruction::Split(stake_lamports + 1)).unwrap(),
                transaction_accounts.clone(),
                instruction_accounts.clone(),
                Some(&sysvar_cache_override),
                Err(InstructionError::InsufficientFunds),
            );

            // should pass
            let accounts = process_instruction_with_sysvar_cache(
                &serialize(&StakeInstruction::Split(stake_lamports / 2)).unwrap(),
                transaction_accounts.clone(),
                instruction_accounts.clone(),
                Some(&sysvar_cache_override),
                Ok(()),
            );
            // no lamport leakage
            assert_eq!(
                accounts[0].lamports() + accounts[1].lamports(),
                stake_lamports
            );

            assert_eq!(from(&accounts[0]).unwrap(), from(&accounts[1]).unwrap());
            match state {
                StakeState::Initialized(_meta) => {
                    assert_eq!(from(&accounts[0]).unwrap(), state);
                }
                StakeState::Stake(_meta, _stake) => {
                    let stake_0 = from(&accounts[0]).unwrap().stake();
                    assert_eq!(stake_0.unwrap().delegation.stake, stake_lamports / 2);
                }
                _ => unreachable!(),
            }
        }

        // should fail, fake owner of destination
        let split_to_account = AccountSharedData::new_data_with_space(
            0,
            &StakeState::Uninitialized,
            std::mem::size_of::<StakeState>(),
            &solana_sdk::pubkey::new_rand(),
        )
        .unwrap();
        transaction_accounts[1] = (split_to_address, split_to_account);
        process_instruction(
            &serialize(&StakeInstruction::Split(stake_lamports / 2)).unwrap(),
            transaction_accounts,
            instruction_accounts,
            Err(InstructionError::IncorrectProgramId),
        );
    }

    #[test]
    fn test_withdraw_stake() {
        let recipient_address = solana_sdk::pubkey::new_rand();
        let authority_address = solana_sdk::pubkey::new_rand();
        let custodian_address = solana_sdk::pubkey::new_rand();
        let stake_address = solana_sdk::pubkey::new_rand();
        let minimum_delegation = crate::get_minimum_delegation(&FeatureSet::all_enabled());
        let stake_lamports = minimum_delegation;
        let stake_account = AccountSharedData::new_data_with_space(
            stake_lamports,
            &StakeState::Uninitialized,
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .unwrap();
        let vote_address = solana_sdk::pubkey::new_rand();
        let mut vote_account =
            vote_state::create_account(&vote_address, &solana_sdk::pubkey::new_rand(), 0, 100);
        vote_account
            .set_state(&VoteStateVersions::new_current(VoteState::default()))
            .unwrap();
        let mut transaction_accounts = vec![
            (stake_address, stake_account),
            (vote_address, vote_account),
            (recipient_address, AccountSharedData::default()),
            (
                authority_address,
                AccountSharedData::new(42, 0, &system_program::id()),
            ),
            (custodian_address, AccountSharedData::default()),
            (
                sysvar::clock::id(),
                account::create_account_shared_data_for_test(&Clock::default()),
            ),
            (
                sysvar::rent::id(),
                account::create_account_shared_data_for_test(&Rent::free()),
            ),
            (
                sysvar::stake_history::id(),
                account::create_account_shared_data_for_test(&StakeHistory::default()),
            ),
            (
                stake_config::id(),
                config::create_account(0, &stake_config::Config::default()),
            ),
        ];
        let mut instruction_accounts = vec![
            AccountMeta {
                pubkey: stake_address,
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: recipient_address,
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: sysvar::clock::id(),
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: sysvar::stake_history::id(),
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: stake_address,
                is_signer: true,
                is_writable: false,
            },
        ];

        // should fail, no signer
        instruction_accounts[4].is_signer = false;
        process_instruction(
            &serialize(&StakeInstruction::Withdraw(stake_lamports)).unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Err(InstructionError::MissingRequiredSignature),
        );
        instruction_accounts[4].is_signer = true;

        // should pass, signed keyed account and uninitialized
        let accounts = process_instruction(
            &serialize(&StakeInstruction::Withdraw(stake_lamports)).unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Ok(()),
        );
        assert_eq!(accounts[0].lamports(), 0);
        assert_eq!(from(&accounts[0]).unwrap(), StakeState::Uninitialized);

        // initialize stake
        let lockup = Lockup {
            unix_timestamp: 0,
            epoch: 0,
            custodian: custodian_address,
        };
        let accounts = process_instruction(
            &serialize(&StakeInstruction::Initialize(
                Authorized::auto(&stake_address),
                lockup,
            ))
            .unwrap(),
            transaction_accounts.clone(),
            vec![
                AccountMeta {
                    pubkey: stake_address,
                    is_signer: true,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: sysvar::rent::id(),
                    is_signer: false,
                    is_writable: false,
                },
            ],
            Ok(()),
        );
        transaction_accounts[0] = (stake_address, accounts[0].clone());

        // should fail, signed keyed account and locked up, more than available
        process_instruction(
            &serialize(&StakeInstruction::Withdraw(stake_lamports + 1)).unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Err(InstructionError::InsufficientFunds),
        );

        // Stake some lamports (available lamports for withdrawals will reduce to zero)
        let accounts = process_instruction(
            &serialize(&StakeInstruction::DelegateStake).unwrap(),
            transaction_accounts.clone(),
            vec![
                AccountMeta {
                    pubkey: stake_address,
                    is_signer: true,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: vote_address,
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: sysvar::clock::id(),
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: sysvar::stake_history::id(),
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: stake_config::id(),
                    is_signer: false,
                    is_writable: false,
                },
            ],
            Ok(()),
        );
        transaction_accounts[0] = (stake_address, accounts[0].clone());

        // simulate rewards
        transaction_accounts[0].1.checked_add_lamports(10).unwrap();

        // withdrawal before deactivate works for rewards amount
        process_instruction(
            &serialize(&StakeInstruction::Withdraw(10)).unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Ok(()),
        );

        // withdrawal of rewards fails if not in excess of stake
        process_instruction(
            &serialize(&StakeInstruction::Withdraw(11)).unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Err(InstructionError::InsufficientFunds),
        );

        // deactivate the stake before withdrawal
        let accounts = process_instruction(
            &serialize(&StakeInstruction::Deactivate).unwrap(),
            transaction_accounts.clone(),
            vec![
                AccountMeta {
                    pubkey: stake_address,
                    is_signer: true,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: sysvar::clock::id(),
                    is_signer: false,
                    is_writable: false,
                },
            ],
            Ok(()),
        );
        transaction_accounts[0] = (stake_address, accounts[0].clone());

        // simulate time passing
        let clock = Clock {
            epoch: 100,
            ..Clock::default()
        };
        transaction_accounts[5] = (
            sysvar::clock::id(),
            account::create_account_shared_data_for_test(&clock),
        );

        // Try to withdraw more than what's available
        process_instruction(
            &serialize(&StakeInstruction::Withdraw(stake_lamports + 11)).unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Err(InstructionError::InsufficientFunds),
        );

        // Try to withdraw all lamports
        let accounts = process_instruction(
            &serialize(&StakeInstruction::Withdraw(stake_lamports + 10)).unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Ok(()),
        );
        assert_eq!(accounts[0].lamports(), 0);
        assert_eq!(from(&accounts[0]).unwrap(), StakeState::Uninitialized);

        // overflow
        let rent = Rent::default();
        let rent_exempt_reserve = rent.minimum_balance(std::mem::size_of::<StakeState>());
        let stake_account = AccountSharedData::new_data_with_space(
            1_000_000_000,
            &StakeState::Initialized(Meta {
                rent_exempt_reserve,
                authorized: Authorized {
                    staker: authority_address,
                    withdrawer: authority_address,
                },
                lockup: Lockup::default(),
            }),
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .unwrap();
        transaction_accounts[0] = (stake_address, stake_account.clone());
        transaction_accounts[2] = (recipient_address, stake_account);
        instruction_accounts[4].pubkey = authority_address;
        process_instruction(
            &serialize(&StakeInstruction::Withdraw(u64::MAX - 10)).unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Err(InstructionError::InsufficientFunds),
        );

        // should fail, invalid state
        let stake_account = AccountSharedData::new_data_with_space(
            stake_lamports,
            &StakeState::RewardsPool,
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .unwrap();
        transaction_accounts[0] = (stake_address, stake_account);
        process_instruction(
            &serialize(&StakeInstruction::Withdraw(stake_lamports)).unwrap(),
            transaction_accounts,
            instruction_accounts,
            Err(InstructionError::InvalidAccountData),
        );
    }

    #[test]
    fn test_withdraw_stake_before_warmup() {
        let recipient_address = solana_sdk::pubkey::new_rand();
        let stake_address = solana_sdk::pubkey::new_rand();
        let stake_lamports = 42;
        let total_lamports = 100;
        let stake_account = AccountSharedData::new_data_with_space(
            total_lamports,
            &StakeState::Initialized(Meta::auto(&stake_address)),
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .unwrap();
        let vote_address = solana_sdk::pubkey::new_rand();
        let mut vote_account =
            vote_state::create_account(&vote_address, &solana_sdk::pubkey::new_rand(), 0, 100);
        vote_account
            .set_state(&VoteStateVersions::new_current(VoteState::default()))
            .unwrap();
        let mut clock = Clock {
            epoch: 16,
            ..Clock::default()
        };
        let mut transaction_accounts = vec![
            (stake_address, stake_account),
            (vote_address, vote_account),
            (recipient_address, AccountSharedData::default()),
            (
                sysvar::clock::id(),
                account::create_account_shared_data_for_test(&clock),
            ),
            (
                sysvar::stake_history::id(),
                account::create_account_shared_data_for_test(&StakeHistory::default()),
            ),
            (
                stake_config::id(),
                config::create_account(0, &stake_config::Config::default()),
            ),
        ];
        let instruction_accounts = vec![
            AccountMeta {
                pubkey: stake_address,
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: recipient_address,
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: sysvar::clock::id(),
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: sysvar::stake_history::id(),
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: stake_address,
                is_signer: true,
                is_writable: false,
            },
        ];

        // Stake some lamports (available lamports for withdrawals will reduce to zero)
        let accounts = process_instruction(
            &serialize(&StakeInstruction::DelegateStake).unwrap(),
            transaction_accounts.clone(),
            vec![
                AccountMeta {
                    pubkey: stake_address,
                    is_signer: true,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: vote_address,
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: sysvar::clock::id(),
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: sysvar::stake_history::id(),
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: stake_config::id(),
                    is_signer: false,
                    is_writable: false,
                },
            ],
            Ok(()),
        );
        transaction_accounts[0] = (stake_address, accounts[0].clone());

        // Try to withdraw stake
        let stake_history = create_stake_history_from_delegations(
            None,
            0..clock.epoch,
            &[stake_from(&accounts[0]).unwrap().delegation],
        );
        transaction_accounts[4] = (
            sysvar::stake_history::id(),
            account::create_account_shared_data_for_test(&stake_history),
        );
        clock.epoch = 0;
        transaction_accounts[3] = (
            sysvar::clock::id(),
            account::create_account_shared_data_for_test(&clock),
        );
        process_instruction(
            &serialize(&StakeInstruction::Withdraw(
                total_lamports - stake_lamports + 1,
            ))
            .unwrap(),
            transaction_accounts,
            instruction_accounts,
            Err(InstructionError::InsufficientFunds),
        );
    }

    #[test]
    fn test_withdraw_lockup() {
        let recipient_address = solana_sdk::pubkey::new_rand();
        let custodian_address = solana_sdk::pubkey::new_rand();
        let stake_address = solana_sdk::pubkey::new_rand();
        let total_lamports = 100;
        let mut meta = Meta {
            lockup: Lockup {
                unix_timestamp: 0,
                epoch: 1,
                custodian: custodian_address,
            },
            ..Meta::auto(&stake_address)
        };
        let stake_account = AccountSharedData::new_data_with_space(
            total_lamports,
            &StakeState::Initialized(meta),
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .unwrap();
        let mut clock = Clock::default();
        let mut transaction_accounts = vec![
            (stake_address, stake_account.clone()),
            (recipient_address, AccountSharedData::default()),
            (custodian_address, AccountSharedData::default()),
            (
                sysvar::clock::id(),
                account::create_account_shared_data_for_test(&clock),
            ),
            (
                sysvar::stake_history::id(),
                account::create_account_shared_data_for_test(&StakeHistory::default()),
            ),
        ];
        let mut instruction_accounts = vec![
            AccountMeta {
                pubkey: stake_address,
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: recipient_address,
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: sysvar::clock::id(),
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: sysvar::stake_history::id(),
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: stake_address,
                is_signer: true,
                is_writable: false,
            },
        ];

        // should fail, lockup is still in force
        process_instruction(
            &serialize(&StakeInstruction::Withdraw(total_lamports)).unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Err(StakeError::LockupInForce.into()),
        );

        // should pass
        instruction_accounts.push(AccountMeta {
            pubkey: custodian_address,
            is_signer: true,
            is_writable: false,
        });
        let accounts = process_instruction(
            &serialize(&StakeInstruction::Withdraw(total_lamports)).unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Ok(()),
        );
        assert_eq!(from(&accounts[0]).unwrap(), StakeState::Uninitialized);

        // should pass, custodian is the same as the withdraw authority
        instruction_accounts[5].pubkey = stake_address;
        meta.lockup.custodian = stake_address;
        let stake_account_self_as_custodian = AccountSharedData::new_data_with_space(
            total_lamports,
            &StakeState::Initialized(meta),
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .unwrap();
        transaction_accounts[0] = (stake_address, stake_account_self_as_custodian);
        let accounts = process_instruction(
            &serialize(&StakeInstruction::Withdraw(total_lamports)).unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Ok(()),
        );
        assert_eq!(from(&accounts[0]).unwrap(), StakeState::Uninitialized);
        transaction_accounts[0] = (stake_address, stake_account);

        // should pass, lockup has expired
        instruction_accounts.pop();
        clock.epoch += 1;
        transaction_accounts[3] = (
            sysvar::clock::id(),
            account::create_account_shared_data_for_test(&clock),
        );
        let accounts = process_instruction(
            &serialize(&StakeInstruction::Withdraw(total_lamports)).unwrap(),
            transaction_accounts,
            instruction_accounts,
            Ok(()),
        );
        assert_eq!(from(&accounts[0]).unwrap(), StakeState::Uninitialized);
    }

    #[test]
    fn test_withdraw_rent_exempt() {
        let recipient_address = solana_sdk::pubkey::new_rand();
        let custodian_address = solana_sdk::pubkey::new_rand();
        let stake_address = solana_sdk::pubkey::new_rand();
        let rent = Rent::default();
        let rent_exempt_reserve = rent.minimum_balance(std::mem::size_of::<StakeState>());
        let minimum_delegation = crate::get_minimum_delegation(&FeatureSet::all_enabled());
        let stake_lamports = 7 * minimum_delegation;
        let stake_account = AccountSharedData::new_data_with_space(
            stake_lamports + rent_exempt_reserve,
            &StakeState::Initialized(Meta {
                rent_exempt_reserve,
                ..Meta::auto(&stake_address)
            }),
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .unwrap();
        let transaction_accounts = vec![
            (stake_address, stake_account),
            (recipient_address, AccountSharedData::default()),
            (custodian_address, AccountSharedData::default()),
            (
                sysvar::clock::id(),
                account::create_account_shared_data_for_test(&Clock::default()),
            ),
            (
                sysvar::stake_history::id(),
                account::create_account_shared_data_for_test(&StakeHistory::default()),
            ),
        ];
        let instruction_accounts = vec![
            AccountMeta {
                pubkey: stake_address,
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: recipient_address,
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: sysvar::clock::id(),
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: sysvar::stake_history::id(),
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: stake_address,
                is_signer: true,
                is_writable: false,
            },
        ];

        // should pass, withdrawing account down to minimum balance
        process_instruction(
            &serialize(&StakeInstruction::Withdraw(
                stake_lamports - minimum_delegation,
            ))
            .unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Ok(()),
        );

        // should fail, withdrawing account down to only rent-exempt reserve
        process_instruction(
            &serialize(&StakeInstruction::Withdraw(stake_lamports)).unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Err(InstructionError::InsufficientFunds),
        );

        // should fail, withdrawal that would leave less than rent-exempt reserve
        process_instruction(
            &serialize(&StakeInstruction::Withdraw(
                stake_lamports + minimum_delegation,
            ))
            .unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Err(InstructionError::InsufficientFunds),
        );

        // should pass, withdrawal of complete account
        process_instruction(
            &serialize(&StakeInstruction::Withdraw(
                stake_lamports + rent_exempt_reserve,
            ))
            .unwrap(),
            transaction_accounts,
            instruction_accounts,
            Ok(()),
        );
    }

    #[test]
    fn test_deactivate() {
        let stake_address = solana_sdk::pubkey::new_rand();
        let stake_lamports = 42;
        let stake_account = AccountSharedData::new_data_with_space(
            stake_lamports,
            &StakeState::Initialized(Meta::auto(&stake_address)),
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .unwrap();
        let vote_address = solana_sdk::pubkey::new_rand();
        let mut vote_account =
            vote_state::create_account(&vote_address, &solana_sdk::pubkey::new_rand(), 0, 100);
        vote_account
            .set_state(&VoteStateVersions::new_current(VoteState::default()))
            .unwrap();
        let mut transaction_accounts = vec![
            (stake_address, stake_account),
            (vote_address, vote_account),
            (
                sysvar::clock::id(),
                account::create_account_shared_data_for_test(&Clock::default()),
            ),
            (
                sysvar::stake_history::id(),
                account::create_account_shared_data_for_test(&StakeHistory::default()),
            ),
            (
                stake_config::id(),
                config::create_account(0, &stake_config::Config::default()),
            ),
        ];
        let mut instruction_accounts = vec![
            AccountMeta {
                pubkey: stake_address,
                is_signer: true,
                is_writable: false,
            },
            AccountMeta {
                pubkey: sysvar::clock::id(),
                is_signer: false,
                is_writable: false,
            },
        ];

        // should fail, not signed
        instruction_accounts[0].is_signer = false;
        process_instruction(
            &serialize(&StakeInstruction::Deactivate).unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Err(InstructionError::InvalidAccountData),
        );
        instruction_accounts[0].is_signer = true;

        // should fail, not staked yet
        process_instruction(
            &serialize(&StakeInstruction::Deactivate).unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Err(InstructionError::InvalidAccountData),
        );

        // Staking
        let accounts = process_instruction(
            &serialize(&StakeInstruction::DelegateStake).unwrap(),
            transaction_accounts.clone(),
            vec![
                AccountMeta {
                    pubkey: stake_address,
                    is_signer: true,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: vote_address,
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: sysvar::clock::id(),
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: sysvar::stake_history::id(),
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: stake_config::id(),
                    is_signer: false,
                    is_writable: false,
                },
            ],
            Ok(()),
        );
        transaction_accounts[0] = (stake_address, accounts[0].clone());

        // should pass
        let accounts = process_instruction(
            &serialize(&StakeInstruction::Deactivate).unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Ok(()),
        );
        transaction_accounts[0] = (stake_address, accounts[0].clone());

        // should fail, only works once
        process_instruction(
            &serialize(&StakeInstruction::Deactivate).unwrap(),
            transaction_accounts,
            instruction_accounts,
            Err(StakeError::AlreadyDeactivated.into()),
        );
    }

    #[test]
    fn test_set_lockup() {
        let custodian_address = solana_sdk::pubkey::new_rand();
        let authorized_address = solana_sdk::pubkey::new_rand();
        let stake_address = solana_sdk::pubkey::new_rand();
        let minimum_delegation = crate::get_minimum_delegation(&FeatureSet::all_enabled());
        let stake_lamports = minimum_delegation;
        let stake_account = AccountSharedData::new_data_with_space(
            stake_lamports,
            &StakeState::Uninitialized,
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .unwrap();
        let vote_address = solana_sdk::pubkey::new_rand();
        let mut vote_account =
            vote_state::create_account(&vote_address, &solana_sdk::pubkey::new_rand(), 0, 100);
        vote_account
            .set_state(&VoteStateVersions::new_current(VoteState::default()))
            .unwrap();
        let instruction_data = serialize(&StakeInstruction::SetLockup(LockupArgs {
            unix_timestamp: Some(1),
            epoch: Some(1),
            custodian: Some(custodian_address),
        }))
        .unwrap();
        let mut transaction_accounts = vec![
            (stake_address, stake_account),
            (vote_address, vote_account),
            (authorized_address, AccountSharedData::default()),
            (custodian_address, AccountSharedData::default()),
            (
                sysvar::clock::id(),
                account::create_account_shared_data_for_test(&Clock::default()),
            ),
            (
                sysvar::rent::id(),
                account::create_account_shared_data_for_test(&Rent::free()),
            ),
            (
                sysvar::stake_history::id(),
                account::create_account_shared_data_for_test(&StakeHistory::default()),
            ),
            (
                stake_config::id(),
                config::create_account(0, &stake_config::Config::default()),
            ),
        ];
        let mut instruction_accounts = vec![
            AccountMeta {
                pubkey: stake_address,
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: sysvar::clock::id(),
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: custodian_address,
                is_signer: true,
                is_writable: false,
            },
        ];

        // should fail, wrong state
        process_instruction(
            &instruction_data,
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Err(InstructionError::InvalidAccountData),
        );

        // initialize stake
        let lockup = Lockup {
            unix_timestamp: 1,
            epoch: 1,
            custodian: custodian_address,
        };
        let accounts = process_instruction(
            &serialize(&StakeInstruction::Initialize(
                Authorized::auto(&stake_address),
                lockup,
            ))
            .unwrap(),
            transaction_accounts.clone(),
            vec![
                AccountMeta {
                    pubkey: stake_address,
                    is_signer: true,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: sysvar::rent::id(),
                    is_signer: false,
                    is_writable: false,
                },
            ],
            Ok(()),
        );
        transaction_accounts[0] = (stake_address, accounts[0].clone());

        // should fail, not signed
        instruction_accounts[2].is_signer = false;
        process_instruction(
            &instruction_data,
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Err(InstructionError::MissingRequiredSignature),
        );
        instruction_accounts[2].is_signer = true;

        // should pass
        process_instruction(
            &instruction_data,
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Ok(()),
        );

        // Staking
        let accounts = process_instruction(
            &serialize(&StakeInstruction::DelegateStake).unwrap(),
            transaction_accounts.clone(),
            vec![
                AccountMeta {
                    pubkey: stake_address,
                    is_signer: true,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: vote_address,
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: sysvar::clock::id(),
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: sysvar::stake_history::id(),
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: stake_config::id(),
                    is_signer: false,
                    is_writable: false,
                },
            ],
            Ok(()),
        );
        transaction_accounts[0] = (stake_address, accounts[0].clone());

        // should fail, not signed
        instruction_accounts[2].is_signer = false;
        process_instruction(
            &instruction_data,
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Err(InstructionError::MissingRequiredSignature),
        );
        instruction_accounts[2].is_signer = true;

        // should pass
        process_instruction(
            &instruction_data,
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Ok(()),
        );

        // Lockup in force
        let instruction_data = serialize(&StakeInstruction::SetLockup(LockupArgs {
            unix_timestamp: Some(2),
            epoch: None,
            custodian: None,
        }))
        .unwrap();

        // should fail, authorized withdrawer cannot change it
        instruction_accounts[0].is_signer = true;
        instruction_accounts[2].is_signer = false;
        process_instruction(
            &instruction_data,
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Err(InstructionError::MissingRequiredSignature),
        );
        instruction_accounts[0].is_signer = false;
        instruction_accounts[2].is_signer = true;

        // should pass, custodian can change it
        process_instruction(
            &instruction_data,
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Ok(()),
        );

        // Lockup expired
        let clock = Clock {
            unix_timestamp: UnixTimestamp::MAX,
            epoch: Epoch::MAX,
            ..Clock::default()
        };
        transaction_accounts[3] = (
            sysvar::clock::id(),
            account::create_account_shared_data_for_test(&clock),
        );

        // should fail, custodian cannot change it
        process_instruction(
            &instruction_data,
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Err(InstructionError::MissingRequiredSignature),
        );

        // should pass, authorized withdrawer can change it
        instruction_accounts[0].is_signer = true;
        instruction_accounts[2].is_signer = false;
        process_instruction(
            &instruction_data,
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Ok(()),
        );

        // Change authorized withdrawer
        let accounts = process_instruction(
            &serialize(&StakeInstruction::Authorize(
                authorized_address,
                StakeAuthorize::Withdrawer,
            ))
            .unwrap(),
            transaction_accounts.clone(),
            vec![
                AccountMeta {
                    pubkey: stake_address,
                    is_signer: true,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: sysvar::clock::id(),
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: authorized_address,
                    is_signer: false,
                    is_writable: false,
                },
            ],
            Ok(()),
        );
        transaction_accounts[0] = (stake_address, accounts[0].clone());

        // should fail, previous authorized withdrawer cannot change the lockup anymore
        process_instruction(
            &instruction_data,
            transaction_accounts,
            instruction_accounts,
            Err(InstructionError::MissingRequiredSignature),
        );
    }

    /// Ensure that `initialize()` respects the minimum delegation requirements
    /// - Assert 1: accounts with a balance equal-to the minimum initialize OK
    /// - Assert 2: accounts with a balance less-than the minimum do not initialize
    #[test]
    fn test_initialize_minimum_stake_delegation() {
        let feature_set = FeatureSet::all_enabled();
        let minimum_delegation = crate::get_minimum_delegation(&feature_set);
        let rent = Rent::default();
        let rent_exempt_reserve = rent.minimum_balance(std::mem::size_of::<StakeState>());
        let stake_address = solana_sdk::pubkey::new_rand();
        let instruction_data = serialize(&StakeInstruction::Initialize(
            Authorized::auto(&stake_address),
            Lockup::default(),
        ))
        .unwrap();
        let instruction_accounts = vec![
            AccountMeta {
                pubkey: stake_address,
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: sysvar::rent::id(),
                is_signer: false,
                is_writable: false,
            },
        ];
        for (stake_delegation, expected_result) in [
            (minimum_delegation, Ok(())),
            (
                minimum_delegation - 1,
                Err(InstructionError::InsufficientFunds),
            ),
        ] {
            let stake_account = AccountSharedData::new(
                stake_delegation + rent_exempt_reserve,
                std::mem::size_of::<StakeState>(),
                &id(),
            );
            process_instruction(
                &instruction_data,
                vec![
                    (stake_address, stake_account),
                    (
                        sysvar::rent::id(),
                        account::create_account_shared_data_for_test(&rent),
                    ),
                ],
                instruction_accounts.clone(),
                expected_result,
            );
        }
    }

    /// Ensure that `delegate()` respects the minimum delegation requirements
    /// - Assert 1: delegating an amount equal-to the minimum delegates OK
    /// - Assert 2: delegating an amount less-than the minimum delegates OK
    /// Also test both asserts above over both StakeState::{Initialized and Stake}, since the logic
    /// is slightly different for the variants.
    ///
    /// NOTE: Even though new stake accounts must have a minimum balance that is at least
    /// the minimum delegation (plus rent exempt reserve), the current behavior allows
    /// withdrawing below the minimum delegation, then re-delegating successfully (see
    /// `test_behavior_withdrawal_then_redelegate_with_less_than_minimum_stake_delegation()` for
    /// more information.)
    #[test]
    fn test_delegate_minimum_stake_delegation() {
        let feature_set = FeatureSet::all_enabled();
        let minimum_delegation = crate::get_minimum_delegation(&feature_set);
        let rent = Rent::default();
        let rent_exempt_reserve = rent.minimum_balance(std::mem::size_of::<StakeState>());
        let stake_address = solana_sdk::pubkey::new_rand();
        let meta = Meta {
            rent_exempt_reserve,
            ..Meta::auto(&stake_address)
        };
        let vote_address = solana_sdk::pubkey::new_rand();
        let vote_account =
            vote_state::create_account(&vote_address, &solana_sdk::pubkey::new_rand(), 0, 100);
        let instruction_accounts = vec![
            AccountMeta {
                pubkey: stake_address,
                is_signer: true,
                is_writable: false,
            },
            AccountMeta {
                pubkey: vote_address,
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: sysvar::clock::id(),
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: sysvar::stake_history::id(),
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: stake_config::id(),
                is_signer: false,
                is_writable: false,
            },
        ];
        for (stake_delegation, expected_result) in [
            (minimum_delegation, Ok(())),
            (minimum_delegation - 1, Ok(())),
        ] {
            for stake_state in &[
                StakeState::Initialized(meta),
                just_stake(meta, stake_delegation),
            ] {
                let stake_account = AccountSharedData::new_data_with_space(
                    stake_delegation + rent_exempt_reserve,
                    stake_state,
                    std::mem::size_of::<StakeState>(),
                    &id(),
                )
                .unwrap();
                process_instruction(
                    &serialize(&StakeInstruction::DelegateStake).unwrap(),
                    vec![
                        (stake_address, stake_account),
                        (vote_address, vote_account.clone()),
                        (
                            sysvar::clock::id(),
                            account::create_account_shared_data_for_test(&Clock::default()),
                        ),
                        (
                            sysvar::stake_history::id(),
                            account::create_account_shared_data_for_test(&StakeHistory::default()),
                        ),
                        (
                            stake_config::id(),
                            config::create_account(0, &stake_config::Config::default()),
                        ),
                    ],
                    instruction_accounts.clone(),
                    expected_result.clone(),
                );
            }
        }
    }

    /// Ensure that `split()` respects the minimum delegation requirements.  This applies to
    /// both the source and destination acounts.  Thus, we have four permutations possible based on
    /// if each account's post-split delegation is equal-to (EQ) or less-than (LT) the minimum:
    ///
    ///  source | dest | result
    /// --------+------+--------
    ///  EQ     | EQ   | Ok
    ///  EQ     | LT   | Err
    ///  LT     | EQ   | Err
    ///  LT     | LT   | Err
    #[test]
    fn test_split_minimum_stake_delegation() {
        let feature_set = FeatureSet::all_enabled();
        let minimum_delegation = crate::get_minimum_delegation(&feature_set);
        let rent = Rent::default();
        let rent_exempt_reserve = rent.minimum_balance(std::mem::size_of::<StakeState>());
        let source_address = Pubkey::new_unique();
        let source_meta = Meta {
            rent_exempt_reserve,
            ..Meta::auto(&source_address)
        };
        let dest_address = Pubkey::new_unique();
        let dest_account = AccountSharedData::new_data_with_space(
            0,
            &StakeState::Uninitialized,
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .unwrap();
        let instruction_accounts = vec![
            AccountMeta {
                pubkey: source_address,
                is_signer: true,
                is_writable: false,
            },
            AccountMeta {
                pubkey: dest_address,
                is_signer: false,
                is_writable: false,
            },
        ];
        for (source_stake_delegation, dest_stake_delegation, expected_result) in [
            (minimum_delegation, minimum_delegation, Ok(())),
            (
                minimum_delegation,
                minimum_delegation - 1,
                Err(InstructionError::InsufficientFunds),
            ),
            (
                minimum_delegation - 1,
                minimum_delegation,
                Err(InstructionError::InsufficientFunds),
            ),
            (
                minimum_delegation - 1,
                minimum_delegation - 1,
                Err(InstructionError::InsufficientFunds),
            ),
        ] {
            // The source account's starting balance is equal to *both* the source and dest
            // accounts' *final* balance
            let source_starting_balance =
                source_stake_delegation + dest_stake_delegation + rent_exempt_reserve * 2;
            for source_stake_state in &[
                StakeState::Initialized(source_meta),
                just_stake(source_meta, source_starting_balance - rent_exempt_reserve),
            ] {
                let source_account = AccountSharedData::new_data_with_space(
                    source_starting_balance,
                    source_stake_state,
                    std::mem::size_of::<StakeState>(),
                    &id(),
                )
                .unwrap();
                process_instruction(
                    &serialize(&StakeInstruction::Split(
                        dest_stake_delegation + rent_exempt_reserve,
                    ))
                    .unwrap(),
                    vec![
                        (source_address, source_account),
                        (dest_address, dest_account.clone()),
                        (
                            sysvar::rent::id(),
                            account::create_account_shared_data_for_test(&rent),
                        ),
                    ],
                    instruction_accounts.clone(),
                    expected_result.clone(),
                );
            }
        }
    }

    /// Ensure that splitting the full amount from an account respects the minimum delegation
    /// requirements.  This ensures that we are future-proofing/testing any raises to the minimum
    /// delegation.
    /// - Assert 1: splitting the full amount from an account that has at least the minimum
    ///             delegation is OK
    /// - Assert 2: splitting the full amount from an account that has less than the minimum
    ///             delegation is not OK
    #[test]
    fn test_split_full_amount_minimum_stake_delegation() {
        let feature_set = FeatureSet::all_enabled();
        let minimum_delegation = crate::get_minimum_delegation(&feature_set);
        let rent = Rent::default();
        let rent_exempt_reserve = rent.minimum_balance(std::mem::size_of::<StakeState>());
        let source_address = Pubkey::new_unique();
        let source_meta = Meta {
            rent_exempt_reserve,
            ..Meta::auto(&source_address)
        };
        let dest_address = Pubkey::new_unique();
        let dest_account = AccountSharedData::new_data_with_space(
            0,
            &StakeState::Uninitialized,
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .unwrap();
        let instruction_accounts = vec![
            AccountMeta {
                pubkey: source_address,
                is_signer: true,
                is_writable: false,
            },
            AccountMeta {
                pubkey: dest_address,
                is_signer: false,
                is_writable: false,
            },
        ];
        for (stake_delegation, expected_result) in [
            (minimum_delegation, Ok(())),
            (
                minimum_delegation - 1,
                Err(InstructionError::InsufficientFunds),
            ),
        ] {
            for source_stake_state in &[
                StakeState::Initialized(source_meta),
                just_stake(source_meta, stake_delegation),
            ] {
                let source_account = AccountSharedData::new_data_with_space(
                    stake_delegation + rent_exempt_reserve,
                    source_stake_state,
                    std::mem::size_of::<StakeState>(),
                    &id(),
                )
                .unwrap();
                process_instruction(
                    &serialize(&StakeInstruction::Split(source_account.lamports())).unwrap(),
                    vec![
                        (source_address, source_account),
                        (dest_address, dest_account.clone()),
                        (
                            sysvar::rent::id(),
                            account::create_account_shared_data_for_test(&rent),
                        ),
                    ],
                    instruction_accounts.clone(),
                    expected_result.clone(),
                );
            }
        }
    }

    /// Ensure that `split()` correctly handles prefunded destination accounts.  When a destination
    /// account already has funds, ensure the minimum split amount reduces accordingly.
    #[test]
    fn test_split_destination_minimum_stake_delegation() {
        let feature_set = FeatureSet::all_enabled();
        let minimum_delegation = crate::get_minimum_delegation(&feature_set);
        let rent = Rent::default();
        let rent_exempt_reserve = rent.minimum_balance(std::mem::size_of::<StakeState>());
        let source_address = Pubkey::new_unique();
        let source_meta = Meta {
            rent_exempt_reserve,
            ..Meta::auto(&source_address)
        };
        let dest_address = Pubkey::new_unique();
        let instruction_accounts = vec![
            AccountMeta {
                pubkey: source_address,
                is_signer: true,
                is_writable: false,
            },
            AccountMeta {
                pubkey: dest_address,
                is_signer: false,
                is_writable: false,
            },
        ];
        for (destination_starting_balance, split_amount, expected_result) in [
            // split amount must be non zero
            (
                rent_exempt_reserve + minimum_delegation,
                0,
                Err(InstructionError::InsufficientFunds),
            ),
            // any split amount is OK when destination account is already fully funded
            (rent_exempt_reserve + minimum_delegation, 1, Ok(())),
            // if destination is only short by 1 lamport, then split amount can be 1 lamport
            (rent_exempt_reserve + minimum_delegation - 1, 1, Ok(())),
            // destination short by 2 lamports, so 1 isn't enough (non-zero split amount)
            (
                rent_exempt_reserve + minimum_delegation - 2,
                1,
                Err(InstructionError::InsufficientFunds),
            ),
            // destination is rent exempt, so split enough for minimum delegation
            (rent_exempt_reserve, minimum_delegation, Ok(())),
            // destination is rent exempt, but split amount less than minimum delegation
            (
                rent_exempt_reserve,
                minimum_delegation - 1,
                Err(InstructionError::InsufficientFunds),
            ),
            // destination is not rent exempt, so split enough for rent and minimum delegation
            (rent_exempt_reserve - 1, minimum_delegation + 1, Ok(())),
            // destination is not rent exempt, but split amount only for minimum delegation
            (
                rent_exempt_reserve - 1,
                minimum_delegation,
                Err(InstructionError::InsufficientFunds),
            ),
            // destination has smallest non-zero balance, so can split the minimum balance
            // requirements minus what destination already has
            (1, rent_exempt_reserve + minimum_delegation - 1, Ok(())),
            // destination has smallest non-zero balance, but cannot split less than the minimum
            // balance requirements minus what destination already has
            (
                1,
                rent_exempt_reserve + minimum_delegation - 2,
                Err(InstructionError::InsufficientFunds),
            ),
            // destination has zero lamports, so split must be at least rent exempt reserve plus
            // minimum delegation
            (0, rent_exempt_reserve + minimum_delegation, Ok(())),
            // destination has zero lamports, but split amount is less than rent exempt reserve
            // plus minimum delegation
            (
                0,
                rent_exempt_reserve + minimum_delegation - 1,
                Err(InstructionError::InsufficientFunds),
            ),
        ] {
            // Set the source's starting balance and stake delegation amount to something large
            // to ensure its post-split balance meets all the requirements
            let source_balance = u64::MAX;
            let source_stake_delegation = source_balance - rent_exempt_reserve;
            for source_stake_state in &[
                StakeState::Initialized(source_meta),
                just_stake(source_meta, source_stake_delegation),
            ] {
                let source_account = AccountSharedData::new_data_with_space(
                    source_balance,
                    &source_stake_state,
                    std::mem::size_of::<StakeState>(),
                    &id(),
                )
                .unwrap();
                let dest_account = AccountSharedData::new_data_with_space(
                    destination_starting_balance,
                    &StakeState::Uninitialized,
                    std::mem::size_of::<StakeState>(),
                    &id(),
                )
                .unwrap();
                let accounts = process_instruction(
                    &serialize(&StakeInstruction::Split(split_amount)).unwrap(),
                    vec![
                        (source_address, source_account),
                        (dest_address, dest_account),
                        (
                            sysvar::rent::id(),
                            account::create_account_shared_data_for_test(&rent),
                        ),
                    ],
                    instruction_accounts.clone(),
                    expected_result.clone(),
                );
                // For the expected OK cases, when the source's StakeState is Stake, then the
                // destination's StakeState *must* also end up as Stake as well.  Additionally,
                // check to ensure the destination's delegation amount is correct.  If the
                // destination is already rent exempt, then the destination's stake delegation
                // *must* equal the split amount. Otherwise, the split amount must first be used to
                // make the destination rent exempt, and then the leftover lamports are delegated.
                if expected_result.is_ok() {
                    if let StakeState::Stake(_, _) = accounts[0].state().unwrap() {
                        if let StakeState::Stake(_, destination_stake) =
                            accounts[1].state().unwrap()
                        {
                            let destination_initial_rent_deficit =
                                rent_exempt_reserve.saturating_sub(destination_starting_balance);
                            let expected_destination_stake_delegation =
                                split_amount - destination_initial_rent_deficit;
                            assert_eq!(
                                expected_destination_stake_delegation,
                                destination_stake.delegation.stake
                            );
                            assert!(destination_stake.delegation.stake >= minimum_delegation,);
                        } else {
                            panic!("destination state must be StakeStake::Stake after successful split when source is also StakeState::Stake!");
                        }
                    }
                }
            }
        }
    }

    /// Ensure that `withdraw()` respects the minimum delegation requirements
    /// - Assert 1: withdrawing so remaining stake is equal-to the minimum is OK
    /// - Assert 2: withdrawing so remaining stake is less-than the minimum is not OK
    #[test]
    fn test_withdraw_minimum_stake_delegation() {
        let feature_set = FeatureSet::all_enabled();
        let minimum_delegation = crate::get_minimum_delegation(&feature_set);
        let rent = Rent::default();
        let rent_exempt_reserve = rent.minimum_balance(std::mem::size_of::<StakeState>());
        let stake_address = solana_sdk::pubkey::new_rand();
        let meta = Meta {
            rent_exempt_reserve,
            ..Meta::auto(&stake_address)
        };
        let recipient_address = solana_sdk::pubkey::new_rand();
        let instruction_accounts = vec![
            AccountMeta {
                pubkey: stake_address,
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: recipient_address,
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: sysvar::clock::id(),
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: sysvar::stake_history::id(),
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: stake_address,
                is_signer: true,
                is_writable: false,
            },
        ];
        let starting_stake_delegation = minimum_delegation;
        for (ending_stake_delegation, expected_result) in [
            (minimum_delegation, Ok(())),
            (
                minimum_delegation - 1,
                Err(InstructionError::InsufficientFunds),
            ),
        ] {
            for stake_state in &[
                StakeState::Initialized(meta),
                just_stake(meta, starting_stake_delegation),
            ] {
                let rewards_balance = 123;
                let stake_account = AccountSharedData::new_data_with_space(
                    starting_stake_delegation + rent_exempt_reserve + rewards_balance,
                    stake_state,
                    std::mem::size_of::<StakeState>(),
                    &id(),
                )
                .unwrap();
                let withdraw_amount =
                    (starting_stake_delegation + rewards_balance) - ending_stake_delegation;
                process_instruction(
                    &serialize(&StakeInstruction::Withdraw(withdraw_amount)).unwrap(),
                    vec![
                        (stake_address, stake_account),
                        (
                            recipient_address,
                            AccountSharedData::new(rent_exempt_reserve, 0, &system_program::id()),
                        ),
                        (
                            sysvar::clock::id(),
                            account::create_account_shared_data_for_test(&Clock::default()),
                        ),
                        (
                            sysvar::rent::id(),
                            account::create_account_shared_data_for_test(&Rent::free()),
                        ),
                        (
                            sysvar::stake_history::id(),
                            account::create_account_shared_data_for_test(&StakeHistory::default()),
                        ),
                        (
                            stake_config::id(),
                            config::create_account(0, &stake_config::Config::default()),
                        ),
                    ],
                    instruction_accounts.clone(),
                    expected_result.clone(),
                );
            }
        }
    }

    /// The stake program currently allows delegations below the minimum stake delegation (see also
    /// `test_delegate_minimum_stake_delegation()`).  This is not the ultimate desired behavior,
    /// but this test ensures the existing behavior is not changed inadvertently.
    ///
    /// This test:
    /// 1. Initialises a stake account (with sufficient balance for both rent and minimum delegation)
    /// 2. Delegates the minimum amount
    /// 3. Deactives the delegation
    /// 4. Withdraws from the account such that the ending balance is *below* rent + minimum delegation
    /// 5. Re-delegates, now with less than the minimum delegation, but it still succeeds
    #[test]
    fn test_behavior_withdrawal_then_redelegate_with_less_than_minimum_stake_delegation() {
        let feature_set = FeatureSet::all_enabled();
        let minimum_delegation = crate::get_minimum_delegation(&feature_set);
        let rent = Rent::default();
        let rent_exempt_reserve = rent.minimum_balance(std::mem::size_of::<StakeState>());
        let stake_address = solana_sdk::pubkey::new_rand();
        let stake_account = AccountSharedData::new(
            rent_exempt_reserve + minimum_delegation,
            std::mem::size_of::<StakeState>(),
            &id(),
        );
        let vote_address = solana_sdk::pubkey::new_rand();
        let vote_account =
            vote_state::create_account(&vote_address, &solana_sdk::pubkey::new_rand(), 0, 100);
        let recipient_address = solana_sdk::pubkey::new_rand();
        let mut clock = Clock::default();
        let mut transaction_accounts = vec![
            (stake_address, stake_account),
            (vote_address, vote_account),
            (
                recipient_address,
                AccountSharedData::new(rent_exempt_reserve, 0, &system_program::id()),
            ),
            (
                sysvar::clock::id(),
                account::create_account_shared_data_for_test(&clock),
            ),
            (
                sysvar::stake_history::id(),
                account::create_account_shared_data_for_test(&StakeHistory::default()),
            ),
            (
                stake_config::id(),
                config::create_account(0, &stake_config::Config::default()),
            ),
            (
                sysvar::rent::id(),
                account::create_account_shared_data_for_test(&rent),
            ),
        ];
        let instruction_accounts = vec![
            AccountMeta {
                pubkey: stake_address,
                is_signer: true,
                is_writable: false,
            },
            AccountMeta {
                pubkey: vote_address,
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: sysvar::clock::id(),
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: sysvar::stake_history::id(),
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: stake_config::id(),
                is_signer: false,
                is_writable: false,
            },
        ];

        let accounts = process_instruction(
            &serialize(&StakeInstruction::Initialize(
                Authorized::auto(&stake_address),
                Lockup::default(),
            ))
            .unwrap(),
            transaction_accounts.clone(),
            vec![
                AccountMeta {
                    pubkey: stake_address,
                    is_signer: true,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: sysvar::rent::id(),
                    is_signer: false,
                    is_writable: false,
                },
            ],
            Ok(()),
        );
        transaction_accounts[0] = (stake_address, accounts[0].clone());

        let accounts = process_instruction(
            &serialize(&StakeInstruction::DelegateStake).unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Ok(()),
        );
        transaction_accounts[0] = (stake_address, accounts[0].clone());
        transaction_accounts[1] = (vote_address, accounts[1].clone());

        clock.epoch += 1;
        transaction_accounts[3] = (
            sysvar::clock::id(),
            account::create_account_shared_data_for_test(&clock),
        );
        let accounts = process_instruction(
            &serialize(&StakeInstruction::Deactivate).unwrap(),
            transaction_accounts.clone(),
            vec![
                AccountMeta {
                    pubkey: stake_address,
                    is_signer: true,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: sysvar::clock::id(),
                    is_signer: false,
                    is_writable: false,
                },
            ],
            Ok(()),
        );
        transaction_accounts[0] = (stake_address, accounts[0].clone());

        clock.epoch += 1;
        transaction_accounts[3] = (
            sysvar::clock::id(),
            account::create_account_shared_data_for_test(&clock),
        );
        let withdraw_amount =
            accounts[0].lamports() - (rent_exempt_reserve + minimum_delegation - 1);
        process_instruction(
            &serialize(&StakeInstruction::Withdraw(withdraw_amount)).unwrap(),
            transaction_accounts.clone(),
            vec![
                AccountMeta {
                    pubkey: stake_address,
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: recipient_address,
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: sysvar::clock::id(),
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: sysvar::stake_history::id(),
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: stake_address,
                    is_signer: true,
                    is_writable: false,
                },
            ],
            Ok(()),
        );

        process_instruction(
            &serialize(&StakeInstruction::DelegateStake).unwrap(),
            transaction_accounts,
            instruction_accounts,
            Ok(()),
        );
    }

    #[test]
    fn test_split_source_uninitialized() {
        let rent = Rent::default();
        let rent_exempt_reserve = rent.minimum_balance(std::mem::size_of::<StakeState>());
        let minimum_delegation = crate::get_minimum_delegation(&FeatureSet::all_enabled());
        let stake_lamports = (rent_exempt_reserve + minimum_delegation) * 2;
        let stake_address = solana_sdk::pubkey::new_rand();
        let stake_account = AccountSharedData::new_data_with_space(
            stake_lamports,
            &StakeState::Uninitialized,
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .unwrap();
        let split_to_address = solana_sdk::pubkey::new_rand();
        let split_to_account = AccountSharedData::new_data_with_space(
            0,
            &StakeState::Uninitialized,
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .unwrap();
        let transaction_accounts = vec![
            (stake_address, stake_account),
            (split_to_address, split_to_account),
        ];
        let mut instruction_accounts = vec![
            AccountMeta {
                pubkey: stake_address,
                is_signer: true,
                is_writable: false,
            },
            AccountMeta {
                pubkey: stake_address,
                is_signer: false,
                is_writable: false,
            },
        ];

        // splitting an uninitialized account where the destination is the same as the source
        {
            // splitting should work when...
            // - when split amount is the full balance
            // - when split amount is zero
            // - when split amount is non-zero and less than the full balance
            //
            // and splitting should fail when the split amount is greater than the balance
            process_instruction(
                &serialize(&StakeInstruction::Split(stake_lamports)).unwrap(),
                transaction_accounts.clone(),
                instruction_accounts.clone(),
                Ok(()),
            );
            process_instruction(
                &serialize(&StakeInstruction::Split(0)).unwrap(),
                transaction_accounts.clone(),
                instruction_accounts.clone(),
                Ok(()),
            );
            process_instruction(
                &serialize(&StakeInstruction::Split(stake_lamports / 2)).unwrap(),
                transaction_accounts.clone(),
                instruction_accounts.clone(),
                Ok(()),
            );
            process_instruction(
                &serialize(&StakeInstruction::Split(stake_lamports + 1)).unwrap(),
                transaction_accounts.clone(),
                instruction_accounts.clone(),
                Err(InstructionError::InsufficientFunds),
            );
        }

        // this should work
        instruction_accounts[1].pubkey = split_to_address;
        let accounts = process_instruction(
            &serialize(&StakeInstruction::Split(stake_lamports / 2)).unwrap(),
            transaction_accounts.clone(),
            instruction_accounts.clone(),
            Ok(()),
        );
        assert_eq!(accounts[0].lamports(), accounts[1].lamports());

        // no signers should fail
        instruction_accounts[0].is_signer = false;
        process_instruction(
            &serialize(&StakeInstruction::Split(stake_lamports / 2)).unwrap(),
            transaction_accounts,
            instruction_accounts,
            Err(InstructionError::MissingRequiredSignature),
        );
    }

    #[test]
    fn test_split_split_not_uninitialized() {
        let stake_lamports = 42;
        let stake_address = solana_sdk::pubkey::new_rand();
        let stake_account = AccountSharedData::new_data_with_space(
            stake_lamports,
            &just_stake(Meta::auto(&stake_address), stake_lamports),
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .unwrap();
        let split_to_address = solana_sdk::pubkey::new_rand();
        let instruction_accounts = vec![
            AccountMeta {
                pubkey: stake_address,
                is_signer: true,
                is_writable: false,
            },
            AccountMeta {
                pubkey: stake_address,
                is_signer: false,
                is_writable: false,
            },
        ];

        for split_to_state in &[
            StakeState::Initialized(Meta::default()),
            StakeState::Stake(Meta::default(), Stake::default()),
            StakeState::RewardsPool,
        ] {
            let split_to_account = AccountSharedData::new_data_with_space(
                0,
                split_to_state,
                std::mem::size_of::<StakeState>(),
                &id(),
            )
            .unwrap();
            process_instruction(
                &serialize(&StakeInstruction::Split(stake_lamports / 2)).unwrap(),
                vec![
                    (stake_address, stake_account.clone()),
                    (split_to_address, split_to_account),
                ],
                instruction_accounts.clone(),
                Err(InstructionError::InvalidAccountData),
            );
        }
    }

    #[test]
    fn test_split_more_than_staked() {
        let rent = Rent::default();
        let rent_exempt_reserve = rent.minimum_balance(std::mem::size_of::<StakeState>());
        let minimum_delegation = crate::get_minimum_delegation(&FeatureSet::all_enabled());
        let stake_lamports = (rent_exempt_reserve + minimum_delegation) * 2;
        let stake_address = solana_sdk::pubkey::new_rand();
        let stake_account = AccountSharedData::new_data_with_space(
            stake_lamports,
            &just_stake(
                Meta {
                    rent_exempt_reserve,
                    ..Meta::auto(&stake_address)
                },
                stake_lamports / 2 - 1,
            ),
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .unwrap();
        let split_to_address = solana_sdk::pubkey::new_rand();
        let split_to_account = AccountSharedData::new_data_with_space(
            0,
            &StakeState::Uninitialized,
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .unwrap();
        let transaction_accounts = vec![
            (stake_address, stake_account),
            (split_to_address, split_to_account),
            (
                sysvar::rent::id(),
                account::create_account_shared_data_for_test(&rent),
            ),
        ];
        let instruction_accounts = vec![
            AccountMeta {
                pubkey: stake_address,
                is_signer: true,
                is_writable: false,
            },
            AccountMeta {
                pubkey: split_to_address,
                is_signer: false,
                is_writable: false,
            },
        ];

        process_instruction(
            &serialize(&StakeInstruction::Split(stake_lamports / 2)).unwrap(),
            transaction_accounts,
            instruction_accounts,
            Err(StakeError::InsufficientStake.into()),
        );
    }

    #[test]
    fn test_split_with_rent() {
        let rent = Rent::default();
        let rent_exempt_reserve = rent.minimum_balance(std::mem::size_of::<StakeState>());
        let minimum_delegation = crate::get_minimum_delegation(&FeatureSet::all_enabled());
        let minimum_balance = rent_exempt_reserve + minimum_delegation;
        let stake_lamports = minimum_balance * 2;
        let stake_address = solana_sdk::pubkey::new_rand();
        let split_to_address = solana_sdk::pubkey::new_rand();
        let split_to_account = AccountSharedData::new_data_with_space(
            0,
            &StakeState::Uninitialized,
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .unwrap();
        let instruction_accounts = vec![
            AccountMeta {
                pubkey: stake_address,
                is_signer: true,
                is_writable: false,
            },
            AccountMeta {
                pubkey: split_to_address,
                is_signer: false,
                is_writable: false,
            },
        ];
        let meta = Meta {
            authorized: Authorized::auto(&stake_address),
            rent_exempt_reserve,
            ..Meta::default()
        };

        // test splitting both an Initialized stake and a Staked stake
        for state in &[
            StakeState::Initialized(meta),
            just_stake(meta, stake_lamports - rent_exempt_reserve),
        ] {
            let stake_account = AccountSharedData::new_data_with_space(
                stake_lamports,
                state,
                std::mem::size_of::<StakeState>(),
                &id(),
            )
            .unwrap();
            let mut transaction_accounts = vec![
                (stake_address, stake_account),
                (split_to_address, split_to_account.clone()),
                (
                    sysvar::rent::id(),
                    account::create_account_shared_data_for_test(&rent),
                ),
            ];

            // not enough to make a non-zero stake account
            process_instruction(
                &serialize(&StakeInstruction::Split(rent_exempt_reserve)).unwrap(),
                transaction_accounts.clone(),
                instruction_accounts.clone(),
                Err(InstructionError::InsufficientFunds),
            );

            // doesn't leave enough for initial stake to be non-zero
            process_instruction(
                &serialize(&StakeInstruction::Split(
                    stake_lamports - rent_exempt_reserve,
                ))
                .unwrap(),
                transaction_accounts.clone(),
                instruction_accounts.clone(),
                Err(InstructionError::InsufficientFunds),
            );

            // split account already has way enough lamports
            transaction_accounts[1].1.set_lamports(minimum_balance);
            let accounts = process_instruction(
                &serialize(&StakeInstruction::Split(stake_lamports - minimum_balance)).unwrap(),
                transaction_accounts,
                instruction_accounts.clone(),
                Ok(()),
            );

            // verify no stake leakage in the case of a stake
            if let StakeState::Stake(meta, stake) = state {
                assert_eq!(
                    accounts[1].state(),
                    Ok(StakeState::Stake(
                        *meta,
                        Stake {
                            delegation: Delegation {
                                stake: stake_lamports - minimum_balance,
                                ..stake.delegation
                            },
                            ..*stake
                        }
                    ))
                );
                assert_eq!(accounts[0].lamports(), minimum_balance,);
                assert_eq!(accounts[1].lamports(), stake_lamports,);
            }
        }
    }

    #[test]
    fn test_split_to_account_with_rent_exempt_reserve() {
        let rent = Rent::default();
        let rent_exempt_reserve = rent.minimum_balance(std::mem::size_of::<StakeState>());
        let minimum_delegation = crate::get_minimum_delegation(&FeatureSet::all_enabled());
        let stake_lamports = (rent_exempt_reserve + minimum_delegation) * 2;
        let stake_address = solana_sdk::pubkey::new_rand();
        let meta = Meta {
            authorized: Authorized::auto(&stake_address),
            rent_exempt_reserve,
            ..Meta::default()
        };
        let state = just_stake(meta, stake_lamports - rent_exempt_reserve);
        let stake_account = AccountSharedData::new_data_with_space(
            stake_lamports,
            &state,
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .unwrap();
        let split_to_address = solana_sdk::pubkey::new_rand();
        let instruction_accounts = vec![
            AccountMeta {
                pubkey: stake_address,
                is_signer: true,
                is_writable: false,
            },
            AccountMeta {
                pubkey: split_to_address,
                is_signer: false,
                is_writable: false,
            },
        ];

        // Test various account prefunding, including empty, less than rent_exempt_reserve, exactly
        // rent_exempt_reserve, and more than rent_exempt_reserve. The empty case is not covered in
        // test_split, since that test uses a Meta with rent_exempt_reserve = 0
        let split_lamport_balances = vec![
            0,
            rent_exempt_reserve - 1,
            rent_exempt_reserve,
            rent_exempt_reserve + minimum_delegation - 1,
            rent_exempt_reserve + minimum_delegation,
        ];
        for initial_balance in split_lamport_balances {
            let split_to_account = AccountSharedData::new_data_with_space(
                initial_balance,
                &StakeState::Uninitialized,
                std::mem::size_of::<StakeState>(),
                &id(),
            )
            .unwrap();
            let transaction_accounts = vec![
                (stake_address, stake_account.clone()),
                (split_to_address, split_to_account),
                (
                    sysvar::rent::id(),
                    account::create_account_shared_data_for_test(&rent),
                ),
            ];

            // split more than available fails
            process_instruction(
                &serialize(&StakeInstruction::Split(stake_lamports + 1)).unwrap(),
                transaction_accounts.clone(),
                instruction_accounts.clone(),
                Err(InstructionError::InsufficientFunds),
            );

            // should work
            let accounts = process_instruction(
                &serialize(&StakeInstruction::Split(stake_lamports / 2)).unwrap(),
                transaction_accounts,
                instruction_accounts.clone(),
                Ok(()),
            );
            // no lamport leakage
            assert_eq!(
                accounts[0].lamports() + accounts[1].lamports(),
                stake_lamports + initial_balance,
            );

            if let StakeState::Stake(meta, stake) = state {
                let expected_stake =
                    stake_lamports / 2 - (rent_exempt_reserve.saturating_sub(initial_balance));
                assert_eq!(
                    Ok(StakeState::Stake(
                        meta,
                        Stake {
                            delegation: Delegation {
                                stake: stake_lamports / 2
                                    - (rent_exempt_reserve.saturating_sub(initial_balance)),
                                ..stake.delegation
                            },
                            ..stake
                        }
                    )),
                    accounts[1].state(),
                );
                assert_eq!(
                    accounts[1].lamports(),
                    expected_stake
                        + rent_exempt_reserve
                        + initial_balance.saturating_sub(rent_exempt_reserve),
                );
                assert_eq!(
                    Ok(StakeState::Stake(
                        meta,
                        Stake {
                            delegation: Delegation {
                                stake: stake_lamports / 2 - rent_exempt_reserve,
                                ..stake.delegation
                            },
                            ..stake
                        }
                    )),
                    accounts[0].state(),
                );
            }
        }
    }

    #[test]
    fn test_split_from_larger_sized_account() {
        let rent = Rent::default();
        let source_larger_rent_exempt_reserve =
            rent.minimum_balance(std::mem::size_of::<StakeState>() + 100);
        let split_rent_exempt_reserve = rent.minimum_balance(std::mem::size_of::<StakeState>());
        let minimum_delegation = crate::get_minimum_delegation(&FeatureSet::all_enabled());
        let stake_lamports = (source_larger_rent_exempt_reserve + minimum_delegation) * 2;
        let stake_address = solana_sdk::pubkey::new_rand();
        let meta = Meta {
            authorized: Authorized::auto(&stake_address),
            rent_exempt_reserve: source_larger_rent_exempt_reserve,
            ..Meta::default()
        };
        let state = just_stake(meta, stake_lamports - source_larger_rent_exempt_reserve);
        let stake_account = AccountSharedData::new_data_with_space(
            stake_lamports,
            &state,
            std::mem::size_of::<StakeState>() + 100,
            &id(),
        )
        .unwrap();
        let split_to_address = solana_sdk::pubkey::new_rand();
        let instruction_accounts = vec![
            AccountMeta {
                pubkey: stake_address,
                is_signer: true,
                is_writable: false,
            },
            AccountMeta {
                pubkey: split_to_address,
                is_signer: false,
                is_writable: false,
            },
        ];

        // Test various account prefunding, including empty, less than rent_exempt_reserve, exactly
        // rent_exempt_reserve, and more than rent_exempt_reserve. The empty case is not covered in
        // test_split, since that test uses a Meta with rent_exempt_reserve = 0
        let split_lamport_balances = vec![
            0,
            split_rent_exempt_reserve - 1,
            split_rent_exempt_reserve,
            split_rent_exempt_reserve + minimum_delegation - 1,
            split_rent_exempt_reserve + minimum_delegation,
        ];
        for initial_balance in split_lamport_balances {
            let split_to_account = AccountSharedData::new_data_with_space(
                initial_balance,
                &StakeState::Uninitialized,
                std::mem::size_of::<StakeState>(),
                &id(),
            )
            .unwrap();
            let transaction_accounts = vec![
                (stake_address, stake_account.clone()),
                (split_to_address, split_to_account),
                (
                    sysvar::rent::id(),
                    account::create_account_shared_data_for_test(&rent),
                ),
            ];

            // split more than available fails
            process_instruction(
                &serialize(&StakeInstruction::Split(stake_lamports + 1)).unwrap(),
                transaction_accounts.clone(),
                instruction_accounts.clone(),
                Err(InstructionError::InsufficientFunds),
            );

            // should work
            let accounts = process_instruction(
                &serialize(&StakeInstruction::Split(stake_lamports / 2)).unwrap(),
                transaction_accounts.clone(),
                instruction_accounts.clone(),
                Ok(()),
            );
            // no lamport leakage
            assert_eq!(
                accounts[0].lamports() + accounts[1].lamports(),
                stake_lamports + initial_balance
            );

            if let StakeState::Stake(meta, stake) = state {
                let expected_split_meta = Meta {
                    authorized: Authorized::auto(&stake_address),
                    rent_exempt_reserve: split_rent_exempt_reserve,
                    ..Meta::default()
                };
                let expected_stake = stake_lamports / 2
                    - (split_rent_exempt_reserve.saturating_sub(initial_balance));

                assert_eq!(
                    Ok(StakeState::Stake(
                        expected_split_meta,
                        Stake {
                            delegation: Delegation {
                                stake: expected_stake,
                                ..stake.delegation
                            },
                            ..stake
                        }
                    )),
                    accounts[1].state()
                );
                assert_eq!(
                    accounts[1].lamports(),
                    expected_stake
                        + split_rent_exempt_reserve
                        + initial_balance.saturating_sub(split_rent_exempt_reserve)
                );
                assert_eq!(
                    Ok(StakeState::Stake(
                        meta,
                        Stake {
                            delegation: Delegation {
                                stake: stake_lamports / 2 - source_larger_rent_exempt_reserve,
                                ..stake.delegation
                            },
                            ..stake
                        }
                    )),
                    accounts[0].state()
                );
            }
        }
    }

    #[test]
    fn test_split_from_smaller_sized_account() {
        let rent = Rent::default();
        let source_smaller_rent_exempt_reserve =
            rent.minimum_balance(std::mem::size_of::<StakeState>());
        let split_rent_exempt_reserve =
            rent.minimum_balance(std::mem::size_of::<StakeState>() + 100);
        let stake_lamports = split_rent_exempt_reserve + 1;
        let stake_address = solana_sdk::pubkey::new_rand();
        let meta = Meta {
            authorized: Authorized::auto(&stake_address),
            rent_exempt_reserve: source_smaller_rent_exempt_reserve,
            ..Meta::default()
        };
        let state = just_stake(meta, stake_lamports - source_smaller_rent_exempt_reserve);
        let stake_account = AccountSharedData::new_data_with_space(
            stake_lamports,
            &state,
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .unwrap();
        let split_to_address = solana_sdk::pubkey::new_rand();
        let instruction_accounts = vec![
            AccountMeta {
                pubkey: stake_address,
                is_signer: true,
                is_writable: false,
            },
            AccountMeta {
                pubkey: split_to_address,
                is_signer: false,
                is_writable: false,
            },
        ];

        let split_amount = stake_lamports - (source_smaller_rent_exempt_reserve + 1); // Enough so that split stake is > 0
        let split_lamport_balances = vec![
            0,
            1,
            split_rent_exempt_reserve,
            split_rent_exempt_reserve + 1,
        ];
        for initial_balance in split_lamport_balances {
            let split_to_account = AccountSharedData::new_data_with_space(
                initial_balance,
                &StakeState::Uninitialized,
                std::mem::size_of::<StakeState>() + 100,
                &id(),
            )
            .unwrap();
            let transaction_accounts = vec![
                (stake_address, stake_account.clone()),
                (split_to_address, split_to_account),
                (
                    sysvar::rent::id(),
                    account::create_account_shared_data_for_test(&rent),
                ),
            ];

            // should always return error when splitting to larger account
            process_instruction(
                &serialize(&StakeInstruction::Split(split_amount)).unwrap(),
                transaction_accounts.clone(),
                instruction_accounts.clone(),
                Err(InstructionError::InvalidAccountData),
            );

            // Splitting 100% of source should not make a difference
            process_instruction(
                &serialize(&StakeInstruction::Split(stake_lamports)).unwrap(),
                transaction_accounts,
                instruction_accounts.clone(),
                Err(InstructionError::InvalidAccountData),
            );
        }
    }

    #[test]
    fn test_split_100_percent_of_source() {
        let rent = Rent::default();
        let rent_exempt_reserve = rent.minimum_balance(std::mem::size_of::<StakeState>());
        let minimum_delegation = crate::get_minimum_delegation(&FeatureSet::all_enabled());
        let stake_lamports = rent_exempt_reserve + minimum_delegation;
        let stake_address = solana_sdk::pubkey::new_rand();
        let meta = Meta {
            authorized: Authorized::auto(&stake_address),
            rent_exempt_reserve,
            ..Meta::default()
        };
        let split_to_address = solana_sdk::pubkey::new_rand();
        let split_to_account = AccountSharedData::new_data_with_space(
            0,
            &StakeState::Uninitialized,
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .unwrap();
        let instruction_accounts = vec![
            AccountMeta {
                pubkey: stake_address,
                is_signer: true,
                is_writable: false,
            },
            AccountMeta {
                pubkey: split_to_address,
                is_signer: false,
                is_writable: false,
            },
        ];

        // test splitting both an Initialized stake and a Staked stake
        for state in &[
            StakeState::Initialized(meta),
            just_stake(meta, stake_lamports - rent_exempt_reserve),
        ] {
            let stake_account = AccountSharedData::new_data_with_space(
                stake_lamports,
                &state,
                std::mem::size_of::<StakeState>(),
                &id(),
            )
            .unwrap();
            let transaction_accounts = vec![
                (stake_address, stake_account),
                (split_to_address, split_to_account.clone()),
                (
                    sysvar::rent::id(),
                    account::create_account_shared_data_for_test(&rent),
                ),
            ];

            // split 100% over to dest
            let accounts = process_instruction(
                &serialize(&StakeInstruction::Split(stake_lamports)).unwrap(),
                transaction_accounts,
                instruction_accounts.clone(),
                Ok(()),
            );

            // no lamport leakage
            assert_eq!(
                accounts[0].lamports() + accounts[1].lamports(),
                stake_lamports
            );

            match state {
                StakeState::Initialized(_) => {
                    assert_eq!(Ok(*state), accounts[1].state());
                    assert_eq!(Ok(StakeState::Uninitialized), accounts[0].state());
                }
                StakeState::Stake(meta, stake) => {
                    assert_eq!(
                        Ok(StakeState::Stake(
                            *meta,
                            Stake {
                                delegation: Delegation {
                                    stake: stake_lamports - rent_exempt_reserve,
                                    ..stake.delegation
                                },
                                ..*stake
                            }
                        )),
                        accounts[1].state()
                    );
                    assert_eq!(Ok(StakeState::Uninitialized), accounts[0].state());
                }
                _ => unreachable!(),
            }
        }
    }

    #[test]
    fn test_split_100_percent_of_source_to_account_with_lamports() {
        let rent = Rent::default();
        let rent_exempt_reserve = rent.minimum_balance(std::mem::size_of::<StakeState>());
        let minimum_delegation = crate::get_minimum_delegation(&FeatureSet::all_enabled());
        let stake_lamports = rent_exempt_reserve + minimum_delegation;
        let stake_address = solana_sdk::pubkey::new_rand();
        let meta = Meta {
            authorized: Authorized::auto(&stake_address),
            rent_exempt_reserve,
            ..Meta::default()
        };
        let state = just_stake(meta, stake_lamports - rent_exempt_reserve);
        let stake_account = AccountSharedData::new_data_with_space(
            stake_lamports,
            &state,
            std::mem::size_of::<StakeState>(),
            &id(),
        )
        .unwrap();
        let split_to_address = solana_sdk::pubkey::new_rand();
        let instruction_accounts = vec![
            AccountMeta {
                pubkey: stake_address,
                is_signer: true,
                is_writable: false,
            },
            AccountMeta {
                pubkey: split_to_address,
                is_signer: false,
                is_writable: false,
            },
        ];

        // Test various account prefunding, including empty, less than rent_exempt_reserve, exactly
        // rent_exempt_reserve, and more than rent_exempt_reserve. Technically, the empty case is
        // covered in test_split_100_percent_of_source, but included here as well for readability
        let split_lamport_balances = vec![
            0,
            rent_exempt_reserve - 1,
            rent_exempt_reserve,
            rent_exempt_reserve + minimum_delegation - 1,
            rent_exempt_reserve + minimum_delegation,
        ];
        for initial_balance in split_lamport_balances {
            let split_to_account = AccountSharedData::new_data_with_space(
                initial_balance,
                &StakeState::Uninitialized,
                std::mem::size_of::<StakeState>(),
                &id(),
            )
            .unwrap();
            let transaction_accounts = vec![
                (stake_address, stake_account.clone()),
                (split_to_address, split_to_account),
                (
                    sysvar::rent::id(),
                    account::create_account_shared_data_for_test(&rent),
                ),
            ];

            // split 100% over to dest
            let accounts = process_instruction(
                &serialize(&StakeInstruction::Split(stake_lamports)).unwrap(),
                transaction_accounts,
                instruction_accounts.clone(),
                Ok(()),
            );

            // no lamport leakage
            assert_eq!(
                accounts[0].lamports() + accounts[1].lamports(),
                stake_lamports + initial_balance
            );

            if let StakeState::Stake(meta, stake) = state {
                assert_eq!(
                    Ok(StakeState::Stake(
                        meta,
                        Stake {
                            delegation: Delegation {
                                stake: stake_lamports - rent_exempt_reserve,
                                ..stake.delegation
                            },
                            ..stake
                        }
                    )),
                    accounts[1].state()
                );
                assert_eq!(Ok(StakeState::Uninitialized), accounts[0].state());
            }
        }
    }

    #[test]
    fn test_split_rent_exemptness() {
        let rent = Rent::default();
        let source_rent_exempt_reserve =
            rent.minimum_balance(std::mem::size_of::<StakeState>() + 100);
        let split_rent_exempt_reserve = rent.minimum_balance(std::mem::size_of::<StakeState>());
        let minimum_delegation = crate::get_minimum_delegation(&FeatureSet::all_enabled());
        let stake_lamports = source_rent_exempt_reserve + minimum_delegation;
        let stake_address = solana_sdk::pubkey::new_rand();
        let meta = Meta {
            authorized: Authorized::auto(&stake_address),
            rent_exempt_reserve: source_rent_exempt_reserve,
            ..Meta::default()
        };
        let split_to_address = solana_sdk::pubkey::new_rand();
        let instruction_accounts = vec![
            AccountMeta {
                pubkey: stake_address,
                is_signer: true,
                is_writable: false,
            },
            AccountMeta {
                pubkey: split_to_address,
                is_signer: false,
                is_writable: false,
            },
        ];

        for state in &[
            StakeState::Initialized(meta),
            just_stake(meta, stake_lamports - source_rent_exempt_reserve),
        ] {
            // Test that splitting to a larger account fails
            let stake_account = AccountSharedData::new_data_with_space(
                stake_lamports,
                &state,
                std::mem::size_of::<StakeState>(),
                &id(),
            )
            .unwrap();
            let split_to_account = AccountSharedData::new_data_with_space(
                0,
                &StakeState::Uninitialized,
                std::mem::size_of::<StakeState>() + 10000,
                &id(),
            )
            .unwrap();
            let transaction_accounts = vec![
                (stake_address, stake_account),
                (split_to_address, split_to_account),
                (
                    sysvar::rent::id(),
                    account::create_account_shared_data_for_test(&rent),
                ),
            ];
            process_instruction(
                &serialize(&StakeInstruction::Split(stake_lamports)).unwrap(),
                transaction_accounts,
                instruction_accounts.clone(),
                Err(InstructionError::InvalidAccountData),
            );

            // Test that splitting from a larger account to a smaller one works.
            // Split amount should not matter, assuming other fund criteria are met
            let stake_account = AccountSharedData::new_data_with_space(
                stake_lamports,
                &state,
                std::mem::size_of::<StakeState>() + 100,
                &id(),
            )
            .unwrap();
            let split_to_account = AccountSharedData::new_data_with_space(
                0,
                &StakeState::Uninitialized,
                std::mem::size_of::<StakeState>(),
                &id(),
            )
            .unwrap();
            let transaction_accounts = vec![
                (stake_address, stake_account),
                (split_to_address, split_to_account),
                (
                    sysvar::rent::id(),
                    account::create_account_shared_data_for_test(&rent),
                ),
            ];
            let accounts = process_instruction(
                &serialize(&StakeInstruction::Split(stake_lamports)).unwrap(),
                transaction_accounts,
                instruction_accounts.clone(),
                Ok(()),
            );
            assert_eq!(accounts[1].lamports(), stake_lamports);

            let expected_split_meta = Meta {
                authorized: Authorized::auto(&stake_address),
                rent_exempt_reserve: split_rent_exempt_reserve,
                ..Meta::default()
            };
            match state {
                StakeState::Initialized(_) => {
                    assert_eq!(
                        Ok(StakeState::Initialized(expected_split_meta)),
                        accounts[1].state()
                    );
                    assert_eq!(Ok(StakeState::Uninitialized), accounts[0].state());
                }
                StakeState::Stake(_meta, stake) => {
                    // Expected stake should reflect original stake amount so that extra lamports
                    // from the rent_exempt_reserve inequality do not magically activate
                    let expected_stake = stake_lamports - source_rent_exempt_reserve;

                    assert_eq!(
                        Ok(StakeState::Stake(
                            expected_split_meta,
                            Stake {
                                delegation: Delegation {
                                    stake: expected_stake,
                                    ..stake.delegation
                                },
                                ..*stake
                            }
                        )),
                        accounts[1].state()
                    );
                    assert_eq!(
                        accounts[1].lamports(),
                        expected_stake + source_rent_exempt_reserve,
                    );
                    assert_eq!(Ok(StakeState::Uninitialized), accounts[0].state());
                }
                _ => unreachable!(),
            }
        }
    }
}
