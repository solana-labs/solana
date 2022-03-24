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
    data: &[u8],
    invoke_context: &mut InvokeContext,
) -> Result<(), InstructionError> {
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;
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
            let rent = get_sysvar_with_account_check::rent(
                keyed_account_at_index(keyed_accounts, first_instruction_account + 1)?,
                invoke_context,
            )?;
            me.initialize(&authorized, &lockup, &rent)
        }
        StakeInstruction::Authorize(authorized_pubkey, stake_authorize) => {
            let require_custodian_for_locked_stake_authorize = invoke_context
                .feature_set
                .is_active(&feature_set::require_custodian_for_locked_stake_authorize::id());

            if require_custodian_for_locked_stake_authorize {
                let clock = get_sysvar_with_account_check::clock(
                    keyed_account_at_index(keyed_accounts, first_instruction_account + 1)?,
                    invoke_context,
                )?;
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
            let authority_base =
                keyed_account_at_index(keyed_accounts, first_instruction_account + 1)?;
            let require_custodian_for_locked_stake_authorize = invoke_context
                .feature_set
                .is_active(&feature_set::require_custodian_for_locked_stake_authorize::id());

            if require_custodian_for_locked_stake_authorize {
                let clock = get_sysvar_with_account_check::clock(
                    keyed_account_at_index(keyed_accounts, first_instruction_account + 2)?,
                    invoke_context,
                )?;
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
            let vote = keyed_account_at_index(keyed_accounts, first_instruction_account + 1)?;
            let clock = get_sysvar_with_account_check::clock(
                keyed_account_at_index(keyed_accounts, first_instruction_account + 2)?,
                invoke_context,
            )?;
            let stake_history = get_sysvar_with_account_check::stake_history(
                keyed_account_at_index(keyed_accounts, first_instruction_account + 3)?,
                invoke_context,
            )?;
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
            let split_stake =
                &keyed_account_at_index(keyed_accounts, first_instruction_account + 1)?;
            me.split(lamports, split_stake, &signers)
        }
        StakeInstruction::Merge => {
            let source_stake =
                &keyed_account_at_index(keyed_accounts, first_instruction_account + 1)?;
            let clock = get_sysvar_with_account_check::clock(
                keyed_account_at_index(keyed_accounts, first_instruction_account + 2)?,
                invoke_context,
            )?;
            let stake_history = get_sysvar_with_account_check::stake_history(
                keyed_account_at_index(keyed_accounts, first_instruction_account + 3)?,
                invoke_context,
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
            let to = &keyed_account_at_index(keyed_accounts, first_instruction_account + 1)?;
            let clock = get_sysvar_with_account_check::clock(
                keyed_account_at_index(keyed_accounts, first_instruction_account + 2)?,
                invoke_context,
            )?;
            let stake_history = get_sysvar_with_account_check::stake_history(
                keyed_account_at_index(keyed_accounts, first_instruction_account + 3)?,
                invoke_context,
            )?;
            me.withdraw(
                lamports,
                to,
                &clock,
                &stake_history,
                keyed_account_at_index(keyed_accounts, first_instruction_account + 4)?,
                keyed_account_at_index(keyed_accounts, first_instruction_account + 5).ok(),
            )
        }
        StakeInstruction::Deactivate => {
            let clock = get_sysvar_with_account_check::clock(
                keyed_account_at_index(keyed_accounts, first_instruction_account + 1)?,
                invoke_context,
            )?;
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

                let rent = get_sysvar_with_account_check::rent(
                    keyed_account_at_index(keyed_accounts, first_instruction_account + 1)?,
                    invoke_context,
                )?;
                me.initialize(&authorized, &Lockup::default(), &rent)
            } else {
                Err(InstructionError::InvalidInstructionData)
            }
        }
        StakeInstruction::AuthorizeChecked(stake_authorize) => {
            if invoke_context
                .feature_set
                .is_active(&feature_set::vote_stake_checked_instructions::id())
            {
                let clock = get_sysvar_with_account_check::clock(
                    keyed_account_at_index(keyed_accounts, first_instruction_account + 1)?,
                    invoke_context,
                )?;
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
                let authority_base =
                    keyed_account_at_index(keyed_accounts, first_instruction_account + 1)?;
                let clock = get_sysvar_with_account_check::clock(
                    keyed_account_at_index(keyed_accounts, first_instruction_account + 2)?,
                    invoke_context,
                )?;
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
        solana_program_runtime::invoke_context::mock_process_instruction,
        solana_sdk::{
            account::{self, AccountSharedData, ReadableAccount, WritableAccount},
            account_utils::StateMut,
            clock::{Epoch, UnixTimestamp},
            instruction::{AccountMeta, Instruction},
            pubkey::Pubkey,
            rent::Rent,
            stake::{
                config as stake_config,
                instruction::{self, LockupArgs},
                state::{Authorized, Lockup, StakeAuthorize},
                MINIMUM_STAKE_DELEGATION,
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
        mock_process_instruction(
            &id(),
            Vec::new(),
            instruction_data,
            transaction_accounts,
            instruction_accounts,
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
        let withdrawal_amount = rent_exempt_reserve + MINIMUM_STAKE_DELEGATION;

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
            rent_exempt_reserve + MINIMUM_STAKE_DELEGATION,
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
        let stake_lamports = rent_exempt_reserve + MINIMUM_STAKE_DELEGATION;
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
        let stake_lamports = MINIMUM_STAKE_DELEGATION * 2;
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
            process_instruction(
                &serialize(&StakeInstruction::Split(stake_lamports + 1)).unwrap(),
                transaction_accounts.clone(),
                instruction_accounts.clone(),
                Err(InstructionError::InsufficientFunds),
            );

            // should pass
            let accounts = process_instruction(
                &serialize(&StakeInstruction::Split(stake_lamports / 2)).unwrap(),
                transaction_accounts.clone(),
                instruction_accounts.clone(),
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
        let stake_lamports = MINIMUM_STAKE_DELEGATION;
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
        let stake_lamports = 7 * MINIMUM_STAKE_DELEGATION;
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
                stake_lamports - MINIMUM_STAKE_DELEGATION,
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
                stake_lamports + MINIMUM_STAKE_DELEGATION,
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
        let stake_lamports = MINIMUM_STAKE_DELEGATION;
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
}
