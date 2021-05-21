use crate::{
    config, id,
    stake_state::{Authorized, Lockup, StakeAccount, StakeAuthorize, StakeState},
};
use log::*;
use num_derive::{FromPrimitive, ToPrimitive};
use serde_derive::{Deserialize, Serialize};
use solana_sdk::{
    clock::{Epoch, UnixTimestamp},
    decode_error::DecodeError,
    feature_set,
    instruction::{AccountMeta, Instruction, InstructionError},
    keyed_account::{from_keyed_account, get_signers, keyed_account_at_index},
    process_instruction::InvokeContext,
    program_utils::limited_deserialize,
    pubkey::Pubkey,
    system_instruction,
    sysvar::{self, clock::Clock, rent::Rent, stake_history::StakeHistory},
};
use thiserror::Error;

/// Reasons the stake might have had an error
#[derive(Error, Debug, Clone, PartialEq, FromPrimitive, ToPrimitive)]
pub enum StakeError {
    #[error("not enough credits to redeem")]
    NoCreditsToRedeem,

    #[error("lockup has not yet expired")]
    LockupInForce,

    #[error("stake already deactivated")]
    AlreadyDeactivated,

    #[error("one re-delegation permitted per epoch")]
    TooSoonToRedelegate,

    #[error("split amount is more than is staked")]
    InsufficientStake,

    #[error("stake account with transient stake cannot be merged")]
    MergeTransientStake,

    #[error("stake account merge failed due to different authority, lockups or state")]
    MergeMismatch,

    #[error("custodian address not present")]
    CustodianMissing,

    #[error("custodian signature not present")]
    CustodianSignatureMissing,
}

impl<E> DecodeError<E> for StakeError {
    fn type_of() -> &'static str {
        "StakeError"
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub enum StakeInstruction {
    /// Initialize a stake with lockup and authorization information
    ///
    /// # Account references
    ///   0. [WRITE] Uninitialized stake account
    ///   1. [] Rent sysvar
    ///
    /// Authorized carries pubkeys that must sign staker transactions
    ///   and withdrawer transactions.
    /// Lockup carries information about withdrawal restrictions
    Initialize(Authorized, Lockup),

    /// Authorize a key to manage stake or withdrawal
    ///
    /// # Account references
    ///   0. [WRITE] Stake account to be updated
    ///   1. [] Clock sysvar
    ///   2. [SIGNER] The stake or withdraw authority
    ///   3. Optional: [SIGNER] Lockup authority, if updating StakeAuthorize::Withdrawer before
    ///      lockup expiration
    Authorize(Pubkey, StakeAuthorize),

    /// Delegate a stake to a particular vote account
    ///
    /// # Account references
    ///   0. [WRITE] Initialized stake account to be delegated
    ///   1. [] Vote account to which this stake will be delegated
    ///   2. [] Clock sysvar
    ///   3. [] Stake history sysvar that carries stake warmup/cooldown history
    ///   4. [] Address of config account that carries stake config
    ///   5. [SIGNER] Stake authority
    ///
    /// The entire balance of the staking account is staked.  DelegateStake
    ///   can be called multiple times, but re-delegation is delayed
    ///   by one epoch
    DelegateStake,

    /// Split u64 tokens and stake off a stake account into another stake account.
    ///
    /// # Account references
    ///   0. [WRITE] Stake account to be split; must be in the Initialized or Stake state
    ///   1. [WRITE] Uninitialized stake account that will take the split-off amount
    ///   2. [SIGNER] Stake authority
    Split(u64),

    /// Withdraw unstaked lamports from the stake account
    ///
    /// # Account references
    ///   0. [WRITE] Stake account from which to withdraw
    ///   1. [WRITE] Recipient account
    ///   2. [] Clock sysvar
    ///   3. [] Stake history sysvar that carries stake warmup/cooldown history
    ///   4. [SIGNER] Withdraw authority
    ///   5. Optional: [SIGNER] Lockup authority, if before lockup expiration
    ///
    /// The u64 is the portion of the stake account balance to be withdrawn,
    ///    must be `<= StakeAccount.lamports - staked_lamports`.
    Withdraw(u64),

    /// Deactivates the stake in the account
    ///
    /// # Account references
    ///   0. [WRITE] Delegated stake account
    ///   1. [] Clock sysvar
    ///   2. [SIGNER] Stake authority
    Deactivate,

    /// Set stake lockup
    ///
    /// # Account references
    ///   0. [WRITE] Initialized stake account
    ///   1. [SIGNER] Lockup authority
    SetLockup(LockupArgs),

    /// Merge two stake accounts.
    ///
    /// Both accounts must have identical lockup and authority keys. A merge
    /// is possible between two stakes in the following states with no additional
    /// conditions:
    ///
    /// * two deactivated stakes
    /// * an inactive stake into an activating stake during its activation epoch
    ///
    /// For the following cases, the voter pubkey and vote credits observed must match:
    ///
    /// * two activated stakes
    /// * two activating accounts that share an activation epoch, during the activation epoch
    ///
    /// All other combinations of stake states will fail to merge, including all
    /// "transient" states, where a stake is activating or deactivating with a
    /// non-zero effective stake.
    ///
    /// # Account references
    ///   0. [WRITE] Destination stake account for the merge
    ///   1. [WRITE] Source stake account for to merge.  This account will be drained
    ///   2. [] Clock sysvar
    ///   3. [] Stake history sysvar that carries stake warmup/cooldown history
    ///   4. [SIGNER] Stake authority
    Merge,

    /// Authorize a key to manage stake or withdrawal with a derived key
    ///
    /// # Account references
    ///   0. [WRITE] Stake account to be updated
    ///   1. [SIGNER] Base key of stake or withdraw authority
    ///   2. [] Clock sysvar
    ///   3. Optional: [SIGNER] Lockup authority, if updating StakeAuthorize::Withdrawer before
    ///      lockup expiration
    AuthorizeWithSeed(AuthorizeWithSeedArgs),
}

#[derive(Default, Debug, Serialize, Deserialize, PartialEq, Clone, Copy)]
pub struct LockupArgs {
    pub unix_timestamp: Option<UnixTimestamp>,
    pub epoch: Option<Epoch>,
    pub custodian: Option<Pubkey>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub struct AuthorizeWithSeedArgs {
    pub new_authorized_pubkey: Pubkey,
    pub stake_authorize: StakeAuthorize,
    pub authority_seed: String,
    pub authority_owner: Pubkey,
}

fn initialize(stake_pubkey: &Pubkey, authorized: &Authorized, lockup: &Lockup) -> Instruction {
    Instruction::new_with_bincode(
        id(),
        &StakeInstruction::Initialize(*authorized, *lockup),
        vec![
            AccountMeta::new(*stake_pubkey, false),
            AccountMeta::new_readonly(sysvar::rent::id(), false),
        ],
    )
}

pub fn create_account_with_seed(
    from_pubkey: &Pubkey,
    stake_pubkey: &Pubkey,
    base: &Pubkey,
    seed: &str,
    authorized: &Authorized,
    lockup: &Lockup,
    lamports: u64,
) -> Vec<Instruction> {
    vec![
        system_instruction::create_account_with_seed(
            from_pubkey,
            stake_pubkey,
            base,
            seed,
            lamports,
            std::mem::size_of::<StakeState>() as u64,
            &id(),
        ),
        initialize(stake_pubkey, authorized, lockup),
    ]
}

pub fn create_account(
    from_pubkey: &Pubkey,
    stake_pubkey: &Pubkey,
    authorized: &Authorized,
    lockup: &Lockup,
    lamports: u64,
) -> Vec<Instruction> {
    vec![
        system_instruction::create_account(
            from_pubkey,
            stake_pubkey,
            lamports,
            std::mem::size_of::<StakeState>() as u64,
            &id(),
        ),
        initialize(stake_pubkey, authorized, lockup),
    ]
}

fn _split(
    stake_pubkey: &Pubkey,
    authorized_pubkey: &Pubkey,
    lamports: u64,
    split_stake_pubkey: &Pubkey,
) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*stake_pubkey, false),
        AccountMeta::new(*split_stake_pubkey, false),
        AccountMeta::new_readonly(*authorized_pubkey, true),
    ];

    Instruction::new_with_bincode(id(), &StakeInstruction::Split(lamports), account_metas)
}

pub fn split(
    stake_pubkey: &Pubkey,
    authorized_pubkey: &Pubkey,
    lamports: u64,
    split_stake_pubkey: &Pubkey,
) -> Vec<Instruction> {
    vec![
        system_instruction::create_account(
            authorized_pubkey, // Sending 0, so any signer will suffice
            split_stake_pubkey,
            0,
            std::mem::size_of::<StakeState>() as u64,
            &id(),
        ),
        _split(
            stake_pubkey,
            authorized_pubkey,
            lamports,
            split_stake_pubkey,
        ),
    ]
}

pub fn split_with_seed(
    stake_pubkey: &Pubkey,
    authorized_pubkey: &Pubkey,
    lamports: u64,
    split_stake_pubkey: &Pubkey, // derived using create_with_seed()
    base: &Pubkey,               // base
    seed: &str,                  // seed
) -> Vec<Instruction> {
    vec![
        system_instruction::create_account_with_seed(
            authorized_pubkey, // Sending 0, so any signer will suffice
            split_stake_pubkey,
            base,
            seed,
            0,
            std::mem::size_of::<StakeState>() as u64,
            &id(),
        ),
        _split(
            stake_pubkey,
            authorized_pubkey,
            lamports,
            split_stake_pubkey,
        ),
    ]
}

pub fn merge(
    destination_stake_pubkey: &Pubkey,
    source_stake_pubkey: &Pubkey,
    authorized_pubkey: &Pubkey,
) -> Vec<Instruction> {
    let account_metas = vec![
        AccountMeta::new(*destination_stake_pubkey, false),
        AccountMeta::new(*source_stake_pubkey, false),
        AccountMeta::new_readonly(sysvar::clock::id(), false),
        AccountMeta::new_readonly(sysvar::stake_history::id(), false),
        AccountMeta::new_readonly(*authorized_pubkey, true),
    ];

    vec![Instruction::new_with_bincode(
        id(),
        &StakeInstruction::Merge,
        account_metas,
    )]
}

pub fn create_account_and_delegate_stake(
    from_pubkey: &Pubkey,
    stake_pubkey: &Pubkey,
    vote_pubkey: &Pubkey,
    authorized: &Authorized,
    lockup: &Lockup,
    lamports: u64,
) -> Vec<Instruction> {
    let mut instructions = create_account(from_pubkey, stake_pubkey, authorized, lockup, lamports);
    instructions.push(delegate_stake(
        stake_pubkey,
        &authorized.staker,
        vote_pubkey,
    ));
    instructions
}

pub fn create_account_with_seed_and_delegate_stake(
    from_pubkey: &Pubkey,
    stake_pubkey: &Pubkey,
    base: &Pubkey,
    seed: &str,
    vote_pubkey: &Pubkey,
    authorized: &Authorized,
    lockup: &Lockup,
    lamports: u64,
) -> Vec<Instruction> {
    let mut instructions = create_account_with_seed(
        from_pubkey,
        stake_pubkey,
        base,
        seed,
        authorized,
        lockup,
        lamports,
    );
    instructions.push(delegate_stake(
        stake_pubkey,
        &authorized.staker,
        vote_pubkey,
    ));
    instructions
}

pub fn authorize(
    stake_pubkey: &Pubkey,
    authorized_pubkey: &Pubkey,
    new_authorized_pubkey: &Pubkey,
    stake_authorize: StakeAuthorize,
    custodian_pubkey: Option<&Pubkey>,
) -> Instruction {
    let mut account_metas = vec![
        AccountMeta::new(*stake_pubkey, false),
        AccountMeta::new_readonly(sysvar::clock::id(), false),
        AccountMeta::new_readonly(*authorized_pubkey, true),
    ];

    if let Some(custodian_pubkey) = custodian_pubkey {
        account_metas.push(AccountMeta::new_readonly(*custodian_pubkey, true));
    }

    Instruction::new_with_bincode(
        id(),
        &StakeInstruction::Authorize(*new_authorized_pubkey, stake_authorize),
        account_metas,
    )
}

pub fn authorize_with_seed(
    stake_pubkey: &Pubkey,
    authority_base: &Pubkey,
    authority_seed: String,
    authority_owner: &Pubkey,
    new_authorized_pubkey: &Pubkey,
    stake_authorize: StakeAuthorize,
    custodian_pubkey: Option<&Pubkey>,
) -> Instruction {
    let mut account_metas = vec![
        AccountMeta::new(*stake_pubkey, false),
        AccountMeta::new_readonly(*authority_base, true),
        AccountMeta::new_readonly(sysvar::clock::id(), false),
    ];

    if let Some(custodian_pubkey) = custodian_pubkey {
        account_metas.push(AccountMeta::new_readonly(*custodian_pubkey, true));
    }

    let args = AuthorizeWithSeedArgs {
        new_authorized_pubkey: *new_authorized_pubkey,
        stake_authorize,
        authority_seed,
        authority_owner: *authority_owner,
    };

    Instruction::new_with_bincode(
        id(),
        &StakeInstruction::AuthorizeWithSeed(args),
        account_metas,
    )
}

pub fn delegate_stake(
    stake_pubkey: &Pubkey,
    authorized_pubkey: &Pubkey,
    vote_pubkey: &Pubkey,
) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*stake_pubkey, false),
        AccountMeta::new_readonly(*vote_pubkey, false),
        AccountMeta::new_readonly(sysvar::clock::id(), false),
        AccountMeta::new_readonly(sysvar::stake_history::id(), false),
        AccountMeta::new_readonly(crate::config::id(), false),
        AccountMeta::new_readonly(*authorized_pubkey, true),
    ];
    Instruction::new_with_bincode(id(), &StakeInstruction::DelegateStake, account_metas)
}

pub fn withdraw(
    stake_pubkey: &Pubkey,
    withdrawer_pubkey: &Pubkey,
    to_pubkey: &Pubkey,
    lamports: u64,
    custodian_pubkey: Option<&Pubkey>,
) -> Instruction {
    let mut account_metas = vec![
        AccountMeta::new(*stake_pubkey, false),
        AccountMeta::new(*to_pubkey, false),
        AccountMeta::new_readonly(sysvar::clock::id(), false),
        AccountMeta::new_readonly(sysvar::stake_history::id(), false),
        AccountMeta::new_readonly(*withdrawer_pubkey, true),
    ];

    if let Some(custodian_pubkey) = custodian_pubkey {
        account_metas.push(AccountMeta::new_readonly(*custodian_pubkey, true));
    }

    Instruction::new_with_bincode(id(), &StakeInstruction::Withdraw(lamports), account_metas)
}

pub fn deactivate_stake(stake_pubkey: &Pubkey, authorized_pubkey: &Pubkey) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*stake_pubkey, false),
        AccountMeta::new_readonly(sysvar::clock::id(), false),
        AccountMeta::new_readonly(*authorized_pubkey, true),
    ];
    Instruction::new_with_bincode(id(), &StakeInstruction::Deactivate, account_metas)
}

pub fn set_lockup(
    stake_pubkey: &Pubkey,
    lockup: &LockupArgs,
    custodian_pubkey: &Pubkey,
) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*stake_pubkey, false),
        AccountMeta::new_readonly(*custodian_pubkey, true),
    ];
    Instruction::new_with_bincode(id(), &StakeInstruction::SetLockup(*lockup), account_metas)
}

pub fn process_instruction(
    _program_id: &Pubkey,
    data: &[u8],
    invoke_context: &mut dyn InvokeContext,
) -> Result<(), InstructionError> {
    let keyed_accounts = invoke_context.get_keyed_accounts()?;

    trace!("process_instruction: {:?}", data);
    trace!("keyed_accounts: {:?}", keyed_accounts);

    let signers = get_signers(keyed_accounts);

    let me = &keyed_account_at_index(keyed_accounts, 0)?;

    if me.owner()? != id() {
        if invoke_context.is_feature_active(&feature_set::check_program_owner::id()) {
            return Err(InstructionError::InvalidAccountOwner);
        } else {
            return Err(InstructionError::IncorrectProgramId);
        }
    }

    match limited_deserialize(data)? {
        StakeInstruction::Initialize(authorized, lockup) => me.initialize(
            &authorized,
            &lockup,
            &from_keyed_account::<Rent>(keyed_account_at_index(keyed_accounts, 1)?)?,
        ),
        StakeInstruction::Authorize(authorized_pubkey, stake_authorize) => {
            let require_custodian_for_locked_stake_authorize = invoke_context.is_feature_active(
                &feature_set::require_custodian_for_locked_stake_authorize::id(),
            );

            if require_custodian_for_locked_stake_authorize {
                let clock =
                    from_keyed_account::<Clock>(keyed_account_at_index(keyed_accounts, 1)?)?;
                let _current_authority = keyed_account_at_index(keyed_accounts, 2)?;
                let custodian =
                    keyed_account_at_index(keyed_accounts, 3).map(|ka| ka.unsigned_key());

                me.authorize(
                    &signers,
                    &authorized_pubkey,
                    stake_authorize,
                    require_custodian_for_locked_stake_authorize,
                    &clock,
                    custodian.ok(),
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
            let authority_base = keyed_account_at_index(keyed_accounts, 1)?;
            let require_custodian_for_locked_stake_authorize = invoke_context.is_feature_active(
                &feature_set::require_custodian_for_locked_stake_authorize::id(),
            );

            if require_custodian_for_locked_stake_authorize {
                let clock =
                    from_keyed_account::<Clock>(keyed_account_at_index(keyed_accounts, 2)?)?;
                let custodian =
                    keyed_account_at_index(keyed_accounts, 3).map(|ka| ka.unsigned_key());

                me.authorize_with_seed(
                    &authority_base,
                    &args.authority_seed,
                    &args.authority_owner,
                    &args.new_authorized_pubkey,
                    args.stake_authorize,
                    require_custodian_for_locked_stake_authorize,
                    &clock,
                    custodian.ok(),
                )
            } else {
                me.authorize_with_seed(
                    &authority_base,
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
            let vote = keyed_account_at_index(keyed_accounts, 1)?;

            me.delegate(
                &vote,
                &from_keyed_account::<Clock>(keyed_account_at_index(keyed_accounts, 2)?)?,
                &from_keyed_account::<StakeHistory>(keyed_account_at_index(keyed_accounts, 3)?)?,
                &config::from_keyed_account(keyed_account_at_index(keyed_accounts, 4)?)?,
                &signers,
            )
        }
        StakeInstruction::Split(lamports) => {
            let split_stake = &keyed_account_at_index(keyed_accounts, 1)?;
            me.split(lamports, split_stake, &signers)
        }
        StakeInstruction::Merge => {
            let source_stake = &keyed_account_at_index(keyed_accounts, 1)?;
            me.merge(
                invoke_context,
                source_stake,
                &from_keyed_account::<Clock>(keyed_account_at_index(keyed_accounts, 2)?)?,
                &from_keyed_account::<StakeHistory>(keyed_account_at_index(keyed_accounts, 3)?)?,
                &signers,
            )
        }

        StakeInstruction::Withdraw(lamports) => {
            let to = &keyed_account_at_index(keyed_accounts, 1)?;
            me.withdraw(
                lamports,
                to,
                &from_keyed_account::<Clock>(keyed_account_at_index(keyed_accounts, 2)?)?,
                &from_keyed_account::<StakeHistory>(keyed_account_at_index(keyed_accounts, 3)?)?,
                keyed_account_at_index(keyed_accounts, 4)?,
                keyed_account_at_index(keyed_accounts, 5).ok(),
                invoke_context.is_feature_active(&feature_set::stake_program_v4::id()),
            )
        }
        StakeInstruction::Deactivate => me.deactivate(
            &from_keyed_account::<Clock>(keyed_account_at_index(keyed_accounts, 1)?)?,
            &signers,
        ),

        StakeInstruction::SetLockup(lockup) => me.set_lockup(&lockup, &signers),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bincode::serialize;
    use solana_sdk::{
        account::{self, Account, AccountSharedData, WritableAccount},
        keyed_account::KeyedAccount,
        process_instruction::MockInvokeContext,
        rent::Rent,
        sysvar::stake_history::StakeHistory,
    };
    use std::cell::RefCell;
    use std::str::FromStr;

    fn create_default_account() -> RefCell<AccountSharedData> {
        RefCell::new(AccountSharedData::default())
    }

    fn create_default_stake_account() -> RefCell<AccountSharedData> {
        RefCell::new(AccountSharedData::from(Account {
            owner: id(),
            ..Account::default()
        }))
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

    fn process_instruction(instruction: &Instruction) -> Result<(), InstructionError> {
        let accounts: Vec<_> = instruction
            .accounts
            .iter()
            .map(|meta| {
                RefCell::new(if sysvar::clock::check_id(&meta.pubkey) {
                    account::create_account_shared_data_for_test(&sysvar::clock::Clock::default())
                } else if sysvar::rewards::check_id(&meta.pubkey) {
                    account::create_account_shared_data_for_test(&sysvar::rewards::Rewards::new(
                        0.0,
                    ))
                } else if sysvar::stake_history::check_id(&meta.pubkey) {
                    account::create_account_shared_data_for_test(&StakeHistory::default())
                } else if config::check_id(&meta.pubkey) {
                    config::create_account(0, &config::Config::default())
                } else if sysvar::rent::check_id(&meta.pubkey) {
                    account::create_account_shared_data_for_test(&Rent::default())
                } else if meta.pubkey == invalid_stake_state_pubkey() {
                    AccountSharedData::from(Account {
                        owner: id(),
                        ..Account::default()
                    })
                } else if meta.pubkey == invalid_vote_state_pubkey() {
                    AccountSharedData::from(Account {
                        owner: solana_vote_program::id(),
                        ..Account::default()
                    })
                } else if meta.pubkey == spoofed_stake_state_pubkey() {
                    AccountSharedData::from(Account {
                        owner: spoofed_stake_program_id(),
                        ..Account::default()
                    })
                } else {
                    AccountSharedData::from(Account {
                        owner: id(),
                        ..Account::default()
                    })
                })
            })
            .collect();

        {
            let keyed_accounts: Vec<_> = instruction
                .accounts
                .iter()
                .zip(accounts.iter())
                .map(|(meta, account)| KeyedAccount::new(&meta.pubkey, meta.is_signer, account))
                .collect();
            super::process_instruction(
                &Pubkey::default(),
                &instruction.data,
                &mut MockInvokeContext::new(keyed_accounts),
            )
        }
    }

    #[test]
    fn test_stake_process_instruction() {
        assert_eq!(
            process_instruction(&initialize(
                &Pubkey::default(),
                &Authorized::default(),
                &Lockup::default()
            )),
            Err(InstructionError::InvalidAccountData),
        );
        assert_eq!(
            process_instruction(&authorize(
                &Pubkey::default(),
                &Pubkey::default(),
                &Pubkey::default(),
                StakeAuthorize::Staker,
                None,
            )),
            Err(InstructionError::InvalidAccountData),
        );
        assert_eq!(
            process_instruction(
                &split(
                    &Pubkey::default(),
                    &Pubkey::default(),
                    100,
                    &invalid_stake_state_pubkey(),
                )[1]
            ),
            Err(InstructionError::InvalidAccountData),
        );
        assert_eq!(
            process_instruction(
                &merge(
                    &Pubkey::default(),
                    &invalid_stake_state_pubkey(),
                    &Pubkey::default(),
                )[0]
            ),
            Err(InstructionError::InvalidAccountData),
        );
        assert_eq!(
            process_instruction(
                &split_with_seed(
                    &Pubkey::default(),
                    &Pubkey::default(),
                    100,
                    &invalid_stake_state_pubkey(),
                    &Pubkey::default(),
                    "seed"
                )[1]
            ),
            Err(InstructionError::InvalidAccountData),
        );
        assert_eq!(
            process_instruction(&delegate_stake(
                &Pubkey::default(),
                &Pubkey::default(),
                &invalid_vote_state_pubkey(),
            )),
            Err(InstructionError::InvalidAccountData),
        );
        assert_eq!(
            process_instruction(&withdraw(
                &Pubkey::default(),
                &Pubkey::default(),
                &solana_sdk::pubkey::new_rand(),
                100,
                None,
            )),
            Err(InstructionError::InvalidAccountData),
        );
        assert_eq!(
            process_instruction(&deactivate_stake(&Pubkey::default(), &Pubkey::default())),
            Err(InstructionError::InvalidAccountData),
        );
        assert_eq!(
            process_instruction(&set_lockup(
                &Pubkey::default(),
                &LockupArgs::default(),
                &Pubkey::default()
            )),
            Err(InstructionError::InvalidAccountData),
        );
    }

    #[test]
    fn test_spoofed_stake_accounts() {
        assert_eq!(
            process_instruction(&initialize(
                &spoofed_stake_state_pubkey(),
                &Authorized::default(),
                &Lockup::default()
            )),
            Err(InstructionError::InvalidAccountOwner),
        );
        assert_eq!(
            process_instruction(&authorize(
                &spoofed_stake_state_pubkey(),
                &Pubkey::default(),
                &Pubkey::default(),
                StakeAuthorize::Staker,
                None,
            )),
            Err(InstructionError::InvalidAccountOwner),
        );
        assert_eq!(
            process_instruction(
                &split(
                    &spoofed_stake_state_pubkey(),
                    &Pubkey::default(),
                    100,
                    &Pubkey::default(),
                )[1]
            ),
            Err(InstructionError::InvalidAccountOwner),
        );
        assert_eq!(
            process_instruction(
                &split(
                    &Pubkey::default(),
                    &Pubkey::default(),
                    100,
                    &spoofed_stake_state_pubkey(),
                )[1]
            ),
            Err(InstructionError::IncorrectProgramId),
        );
        assert_eq!(
            process_instruction(
                &merge(
                    &spoofed_stake_state_pubkey(),
                    &Pubkey::default(),
                    &Pubkey::default(),
                )[0]
            ),
            Err(InstructionError::InvalidAccountOwner),
        );
        assert_eq!(
            process_instruction(
                &merge(
                    &Pubkey::default(),
                    &spoofed_stake_state_pubkey(),
                    &Pubkey::default(),
                )[0]
            ),
            Err(InstructionError::IncorrectProgramId),
        );
        assert_eq!(
            process_instruction(
                &split_with_seed(
                    &spoofed_stake_state_pubkey(),
                    &Pubkey::default(),
                    100,
                    &Pubkey::default(),
                    &Pubkey::default(),
                    "seed"
                )[1]
            ),
            Err(InstructionError::InvalidAccountOwner),
        );
        assert_eq!(
            process_instruction(&delegate_stake(
                &spoofed_stake_state_pubkey(),
                &Pubkey::default(),
                &Pubkey::default(),
            )),
            Err(InstructionError::InvalidAccountOwner),
        );
        assert_eq!(
            process_instruction(&withdraw(
                &spoofed_stake_state_pubkey(),
                &Pubkey::default(),
                &solana_sdk::pubkey::new_rand(),
                100,
                None,
            )),
            Err(InstructionError::InvalidAccountOwner),
        );
        assert_eq!(
            process_instruction(&deactivate_stake(
                &spoofed_stake_state_pubkey(),
                &Pubkey::default()
            )),
            Err(InstructionError::InvalidAccountOwner),
        );
        assert_eq!(
            process_instruction(&set_lockup(
                &spoofed_stake_state_pubkey(),
                &LockupArgs::default(),
                &Pubkey::default()
            )),
            Err(InstructionError::InvalidAccountOwner),
        );
    }

    #[test]
    fn test_stake_process_instruction_decode_bail() {
        // these will not call stake_state, have bogus contents

        // gets the "is_empty()" check
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &serialize(&StakeInstruction::Initialize(
                    Authorized::default(),
                    Lockup::default()
                ))
                .unwrap(),
                &mut MockInvokeContext::new(vec![])
            ),
            Err(InstructionError::NotEnoughAccountKeys),
        );

        // no account for rent
        let stake_address = Pubkey::default();
        let stake_account = create_default_stake_account();
        let keyed_accounts = vec![KeyedAccount::new(&stake_address, false, &stake_account)];
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &serialize(&StakeInstruction::Initialize(
                    Authorized::default(),
                    Lockup::default()
                ))
                .unwrap(),
                &mut MockInvokeContext::new(keyed_accounts)
            ),
            Err(InstructionError::NotEnoughAccountKeys),
        );

        // rent fails to deserialize
        let stake_address = Pubkey::default();
        let stake_account = create_default_stake_account();
        let rent_address = sysvar::rent::id();
        let rent_account = create_default_account();
        let keyed_accounts = vec![
            KeyedAccount::new(&stake_address, false, &stake_account),
            KeyedAccount::new(&rent_address, false, &rent_account),
        ];
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &serialize(&StakeInstruction::Initialize(
                    Authorized::default(),
                    Lockup::default()
                ))
                .unwrap(),
                &mut MockInvokeContext::new(keyed_accounts)
            ),
            Err(InstructionError::InvalidArgument),
        );

        // fails to deserialize stake state
        let stake_address = Pubkey::default();
        let stake_account = create_default_stake_account();
        let rent_address = sysvar::rent::id();
        let rent_account = RefCell::new(account::create_account_shared_data_for_test(
            &Rent::default(),
        ));
        let keyed_accounts = vec![
            KeyedAccount::new(&stake_address, false, &stake_account),
            KeyedAccount::new(&rent_address, false, &rent_account),
        ];
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &serialize(&StakeInstruction::Initialize(
                    Authorized::default(),
                    Lockup::default()
                ))
                .unwrap(),
                &mut MockInvokeContext::new(keyed_accounts)
            ),
            Err(InstructionError::InvalidAccountData),
        );

        // gets the first check in delegate, wrong number of accounts
        let stake_address = Pubkey::default();
        let stake_account = create_default_stake_account();
        let keyed_accounts = vec![KeyedAccount::new(&stake_address, false, &stake_account)];
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &serialize(&StakeInstruction::DelegateStake).unwrap(),
                &mut MockInvokeContext::new(keyed_accounts)
            ),
            Err(InstructionError::NotEnoughAccountKeys),
        );

        // gets the sub-check for number of args
        let stake_address = Pubkey::default();
        let stake_account = create_default_stake_account();
        let keyed_accounts = vec![KeyedAccount::new(&stake_address, false, &stake_account)];
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &serialize(&StakeInstruction::DelegateStake).unwrap(),
                &mut MockInvokeContext::new(keyed_accounts)
            ),
            Err(InstructionError::NotEnoughAccountKeys),
        );

        // gets the check non-deserialize-able account in delegate_stake
        let stake_address = Pubkey::default();
        let stake_account = create_default_stake_account();
        let vote_address = Pubkey::default();
        let mut bad_vote_account = create_default_account();
        bad_vote_account
            .get_mut()
            .set_owner(solana_vote_program::id());
        let clock_address = sysvar::clock::id();
        let clock_account = RefCell::new(account::create_account_shared_data_for_test(
            &sysvar::clock::Clock::default(),
        ));
        let stake_history_address = sysvar::stake_history::id();
        let stake_history_account = RefCell::new(account::create_account_shared_data_for_test(
            &sysvar::stake_history::StakeHistory::default(),
        ));
        let config_address = config::id();
        let config_account = RefCell::new(config::create_account(0, &config::Config::default()));
        let keyed_accounts = vec![
            KeyedAccount::new(&stake_address, true, &stake_account),
            KeyedAccount::new(&vote_address, false, &bad_vote_account),
            KeyedAccount::new(&clock_address, false, &clock_account),
            KeyedAccount::new(&stake_history_address, false, &stake_history_account),
            KeyedAccount::new(&config_address, false, &config_account),
        ];
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &serialize(&StakeInstruction::DelegateStake).unwrap(),
                &mut MockInvokeContext::new(keyed_accounts)
            ),
            Err(InstructionError::InvalidAccountData),
        );

        // Tests 3rd keyed account is of correct type (Clock instead of rewards) in withdraw
        let stake_address = Pubkey::default();
        let stake_account = create_default_stake_account();
        let vote_address = Pubkey::default();
        let vote_account = create_default_account();
        let rewards_address = sysvar::rewards::id();
        let rewards_account = RefCell::new(account::create_account_shared_data_for_test(
            &sysvar::rewards::Rewards::new(0.0),
        ));
        let stake_history_address = sysvar::stake_history::id();
        let stake_history_account = RefCell::new(account::create_account_shared_data_for_test(
            &StakeHistory::default(),
        ));
        let keyed_accounts = vec![
            KeyedAccount::new(&stake_address, false, &stake_account),
            KeyedAccount::new(&vote_address, false, &vote_account),
            KeyedAccount::new(&rewards_address, false, &rewards_account),
            KeyedAccount::new(&stake_history_address, false, &stake_history_account),
        ];
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &serialize(&StakeInstruction::Withdraw(42)).unwrap(),
                &mut MockInvokeContext::new(keyed_accounts)
            ),
            Err(InstructionError::InvalidArgument),
        );

        // Tests correct number of accounts are provided in withdraw
        let stake_address = Pubkey::default();
        let stake_account = create_default_stake_account();
        let keyed_accounts = vec![KeyedAccount::new(&stake_address, false, &stake_account)];
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &serialize(&StakeInstruction::Withdraw(42)).unwrap(),
                &mut MockInvokeContext::new(keyed_accounts)
            ),
            Err(InstructionError::NotEnoughAccountKeys),
        );

        // Tests 2nd keyed account is of correct type (Clock instead of rewards) in deactivate
        let stake_address = Pubkey::default();
        let stake_account = create_default_stake_account();
        let rewards_address = sysvar::rewards::id();
        let rewards_account = RefCell::new(account::create_account_shared_data_for_test(
            &sysvar::rewards::Rewards::new(0.0),
        ));
        let keyed_accounts = vec![
            KeyedAccount::new(&stake_address, false, &stake_account),
            KeyedAccount::new(&rewards_address, false, &rewards_account),
        ];
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &serialize(&StakeInstruction::Deactivate).unwrap(),
                &mut MockInvokeContext::new(keyed_accounts)
            ),
            Err(InstructionError::InvalidArgument),
        );

        // Tests correct number of accounts are provided in deactivate
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &serialize(&StakeInstruction::Deactivate).unwrap(),
                &mut MockInvokeContext::new(vec![])
            ),
            Err(InstructionError::NotEnoughAccountKeys),
        );
    }

    #[test]
    fn test_custom_error_decode() {
        use num_traits::FromPrimitive;
        fn pretty_err<T>(err: InstructionError) -> String
        where
            T: 'static + std::error::Error + DecodeError<T> + FromPrimitive,
        {
            if let InstructionError::Custom(code) = err {
                let specific_error: T = T::decode_custom_error_to_enum(code).unwrap();
                format!(
                    "{:?}: {}::{:?} - {}",
                    err,
                    T::type_of(),
                    specific_error,
                    specific_error,
                )
            } else {
                "".to_string()
            }
        }
        assert_eq!(
            "Custom(0): StakeError::NoCreditsToRedeem - not enough credits to redeem",
            pretty_err::<StakeError>(StakeError::NoCreditsToRedeem.into())
        )
    }
}
