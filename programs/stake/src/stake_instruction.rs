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
    instruction::{AccountMeta, Instruction, InstructionError},
    keyed_account::{from_keyed_account, get_signers, next_keyed_account, KeyedAccount},
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

    /// Merge two stake accounts. Both accounts must be deactivated and have identical lockup and
    /// authority keys.
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
    Instruction::new(
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

    Instruction::new(id(), &StakeInstruction::Split(lamports), account_metas)
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
    split_stake_pubkey: &Pubkey, // derived using create_address_with_seed()
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

    vec![Instruction::new(
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

    Instruction::new(
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

    Instruction::new(
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
    Instruction::new(id(), &StakeInstruction::DelegateStake, account_metas)
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

    Instruction::new(id(), &StakeInstruction::Withdraw(lamports), account_metas)
}

pub fn deactivate_stake(stake_pubkey: &Pubkey, authorized_pubkey: &Pubkey) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*stake_pubkey, false),
        AccountMeta::new_readonly(sysvar::clock::id(), false),
        AccountMeta::new_readonly(*authorized_pubkey, true),
    ];
    Instruction::new(id(), &StakeInstruction::Deactivate, account_metas)
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
    Instruction::new(id(), &StakeInstruction::SetLockup(*lockup), account_metas)
}

pub fn process_instruction(
    _program_id: &Pubkey,
    keyed_accounts: &[KeyedAccount],
    data: &[u8],
    _invoke_context: &mut dyn InvokeContext,
) -> Result<(), InstructionError> {
    trace!("process_instruction: {:?}", data);
    trace!("keyed_accounts: {:?}", keyed_accounts);

    let signers = get_signers(keyed_accounts);

    let keyed_accounts = &mut keyed_accounts.iter();
    let me = &next_keyed_account(keyed_accounts)?;

    if me.owner()? != id() {
        return Err(InstructionError::IncorrectProgramId);
    }

    match limited_deserialize(data)? {
        StakeInstruction::Initialize(authorized, lockup) => me.initialize(
            &authorized,
            &lockup,
            &from_keyed_account::<Rent>(next_keyed_account(keyed_accounts)?)?,
        ),
        StakeInstruction::Authorize(authorized_pubkey, stake_authorize) => {
            me.authorize(&signers, &authorized_pubkey, stake_authorize)
        }
        StakeInstruction::AuthorizeWithSeed(args) => {
            let authority_base = next_keyed_account(keyed_accounts)?;
            me.authorize_with_seed(
                &authority_base,
                &args.authority_seed,
                &args.authority_owner,
                &args.new_authorized_pubkey,
                args.stake_authorize,
            )
        }
        StakeInstruction::DelegateStake => {
            let vote = next_keyed_account(keyed_accounts)?;

            me.delegate(
                &vote,
                &from_keyed_account::<Clock>(next_keyed_account(keyed_accounts)?)?,
                &from_keyed_account::<StakeHistory>(next_keyed_account(keyed_accounts)?)?,
                &config::from_keyed_account(next_keyed_account(keyed_accounts)?)?,
                &signers,
            )
        }
        StakeInstruction::Split(lamports) => {
            let split_stake = &next_keyed_account(keyed_accounts)?;
            me.split(lamports, split_stake, &signers)
        }
        StakeInstruction::Merge => {
            let source_stake = &next_keyed_account(keyed_accounts)?;
            me.merge(
                source_stake,
                &from_keyed_account::<Clock>(next_keyed_account(keyed_accounts)?)?,
                &from_keyed_account::<StakeHistory>(next_keyed_account(keyed_accounts)?)?,
                &signers,
            )
        }

        StakeInstruction::Withdraw(lamports) => {
            let to = &next_keyed_account(keyed_accounts)?;
            me.withdraw(
                lamports,
                to,
                &from_keyed_account::<Clock>(next_keyed_account(keyed_accounts)?)?,
                &from_keyed_account::<StakeHistory>(next_keyed_account(keyed_accounts)?)?,
                next_keyed_account(keyed_accounts)?,
                keyed_accounts.next(),
            )
        }
        StakeInstruction::Deactivate => me.deactivate(
            &from_keyed_account::<Clock>(next_keyed_account(keyed_accounts)?)?,
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
        account::{self, Account},
        process_instruction::MockInvokeContext,
        rent::Rent,
        sysvar::stake_history::StakeHistory,
    };
    use std::cell::RefCell;
    use std::str::FromStr;

    fn create_default_account() -> RefCell<Account> {
        RefCell::new(Account::default())
    }

    fn create_default_stake_account() -> RefCell<Account> {
        RefCell::new(Account {
            owner: id(),
            ..Account::default()
        })
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
                    account::create_account(&sysvar::clock::Clock::default(), 1)
                } else if sysvar::rewards::check_id(&meta.pubkey) {
                    account::create_account(&sysvar::rewards::Rewards::new(0.0), 1)
                } else if sysvar::stake_history::check_id(&meta.pubkey) {
                    account::create_account(&StakeHistory::default(), 1)
                } else if config::check_id(&meta.pubkey) {
                    config::create_account(0, &config::Config::default())
                } else if sysvar::rent::check_id(&meta.pubkey) {
                    account::create_account(&Rent::default(), 1)
                } else if meta.pubkey == invalid_stake_state_pubkey() {
                    Account {
                        owner: id(),
                        ..Account::default()
                    }
                } else if meta.pubkey == invalid_vote_state_pubkey() {
                    Account {
                        owner: solana_vote_program::id(),
                        ..Account::default()
                    }
                } else if meta.pubkey == spoofed_stake_state_pubkey() {
                    Account {
                        owner: spoofed_stake_program_id(),
                        ..Account::default()
                    }
                } else {
                    Account {
                        owner: id(),
                        ..Account::default()
                    }
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
                &keyed_accounts,
                &instruction.data,
                &mut MockInvokeContext::default(),
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
            Err(InstructionError::IncorrectProgramId),
        );
        assert_eq!(
            process_instruction(&authorize(
                &spoofed_stake_state_pubkey(),
                &Pubkey::default(),
                &Pubkey::default(),
                StakeAuthorize::Staker,
                None,
            )),
            Err(InstructionError::IncorrectProgramId),
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
            Err(InstructionError::IncorrectProgramId),
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
            Err(InstructionError::IncorrectProgramId),
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
            Err(InstructionError::IncorrectProgramId),
        );
        assert_eq!(
            process_instruction(&delegate_stake(
                &spoofed_stake_state_pubkey(),
                &Pubkey::default(),
                &Pubkey::default(),
            )),
            Err(InstructionError::IncorrectProgramId),
        );
        assert_eq!(
            process_instruction(&withdraw(
                &spoofed_stake_state_pubkey(),
                &Pubkey::default(),
                &solana_sdk::pubkey::new_rand(),
                100,
                None,
            )),
            Err(InstructionError::IncorrectProgramId),
        );
        assert_eq!(
            process_instruction(&deactivate_stake(
                &spoofed_stake_state_pubkey(),
                &Pubkey::default()
            )),
            Err(InstructionError::IncorrectProgramId),
        );
        assert_eq!(
            process_instruction(&set_lockup(
                &spoofed_stake_state_pubkey(),
                &LockupArgs::default(),
                &Pubkey::default()
            )),
            Err(InstructionError::IncorrectProgramId),
        );
    }

    #[test]
    fn test_stake_process_instruction_decode_bail() {
        // these will not call stake_state, have bogus contents

        // gets the "is_empty()" check
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &[],
                &serialize(&StakeInstruction::Initialize(
                    Authorized::default(),
                    Lockup::default()
                ))
                .unwrap(),
                &mut MockInvokeContext::default()
            ),
            Err(InstructionError::NotEnoughAccountKeys),
        );

        // no account for rent
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &[KeyedAccount::new(
                    &Pubkey::default(),
                    false,
                    &create_default_stake_account(),
                )],
                &serialize(&StakeInstruction::Initialize(
                    Authorized::default(),
                    Lockup::default()
                ))
                .unwrap(),
                &mut MockInvokeContext::default()
            ),
            Err(InstructionError::NotEnoughAccountKeys),
        );

        // rent fails to deserialize
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &[
                    KeyedAccount::new(&Pubkey::default(), false, &create_default_stake_account()),
                    KeyedAccount::new(&sysvar::rent::id(), false, &create_default_account())
                ],
                &serialize(&StakeInstruction::Initialize(
                    Authorized::default(),
                    Lockup::default()
                ))
                .unwrap(),
                &mut MockInvokeContext::default()
            ),
            Err(InstructionError::InvalidArgument),
        );

        // fails to deserialize stake state
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &[
                    KeyedAccount::new(&Pubkey::default(), false, &create_default_stake_account()),
                    KeyedAccount::new(
                        &sysvar::rent::id(),
                        false,
                        &RefCell::new(account::create_account(&Rent::default(), 0))
                    )
                ],
                &serialize(&StakeInstruction::Initialize(
                    Authorized::default(),
                    Lockup::default()
                ))
                .unwrap(),
                &mut MockInvokeContext::default()
            ),
            Err(InstructionError::InvalidAccountData),
        );

        // gets the first check in delegate, wrong number of accounts
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &[KeyedAccount::new(
                    &Pubkey::default(),
                    false,
                    &create_default_stake_account()
                ),],
                &serialize(&StakeInstruction::DelegateStake).unwrap(),
                &mut MockInvokeContext::default()
            ),
            Err(InstructionError::NotEnoughAccountKeys),
        );

        // gets the sub-check for number of args
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &[KeyedAccount::new(
                    &Pubkey::default(),
                    false,
                    &create_default_stake_account()
                )],
                &serialize(&StakeInstruction::DelegateStake).unwrap(),
                &mut MockInvokeContext::default()
            ),
            Err(InstructionError::NotEnoughAccountKeys),
        );

        // gets the check non-deserialize-able account in delegate_stake
        let mut bad_vote_account = create_default_account();
        bad_vote_account.get_mut().owner = solana_vote_program::id();
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &[
                    KeyedAccount::new(&Pubkey::default(), true, &create_default_stake_account()),
                    KeyedAccount::new(&Pubkey::default(), false, &bad_vote_account),
                    KeyedAccount::new(
                        &sysvar::clock::id(),
                        false,
                        &RefCell::new(account::create_account(&sysvar::clock::Clock::default(), 1))
                    ),
                    KeyedAccount::new(
                        &sysvar::stake_history::id(),
                        false,
                        &RefCell::new(account::create_account(
                            &sysvar::stake_history::StakeHistory::default(),
                            1
                        ))
                    ),
                    KeyedAccount::new(
                        &config::id(),
                        false,
                        &RefCell::new(config::create_account(0, &config::Config::default()))
                    ),
                ],
                &serialize(&StakeInstruction::DelegateStake).unwrap(),
                &mut MockInvokeContext::default()
            ),
            Err(InstructionError::InvalidAccountData),
        );

        // Tests 3rd keyed account is of correct type (Clock instead of rewards) in withdraw
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &[
                    KeyedAccount::new(&Pubkey::default(), false, &create_default_stake_account()),
                    KeyedAccount::new(&Pubkey::default(), false, &create_default_account()),
                    KeyedAccount::new(
                        &sysvar::rewards::id(),
                        false,
                        &RefCell::new(account::create_account(
                            &sysvar::rewards::Rewards::new(0.0),
                            1
                        ))
                    ),
                    KeyedAccount::new(
                        &sysvar::stake_history::id(),
                        false,
                        &RefCell::new(account::create_account(&StakeHistory::default(), 1,))
                    ),
                ],
                &serialize(&StakeInstruction::Withdraw(42)).unwrap(),
                &mut MockInvokeContext::default()
            ),
            Err(InstructionError::InvalidArgument),
        );

        // Tests correct number of accounts are provided in withdraw
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &[KeyedAccount::new(
                    &Pubkey::default(),
                    false,
                    &create_default_stake_account()
                )],
                &serialize(&StakeInstruction::Withdraw(42)).unwrap(),
                &mut MockInvokeContext::default()
            ),
            Err(InstructionError::NotEnoughAccountKeys),
        );

        // Tests 2nd keyed account is of correct type (Clock instead of rewards) in deactivate
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &[
                    KeyedAccount::new(&Pubkey::default(), false, &create_default_stake_account()),
                    KeyedAccount::new(
                        &sysvar::rewards::id(),
                        false,
                        &RefCell::new(account::create_account(
                            &sysvar::rewards::Rewards::new(0.0),
                            1
                        ))
                    ),
                ],
                &serialize(&StakeInstruction::Deactivate).unwrap(),
                &mut MockInvokeContext::default()
            ),
            Err(InstructionError::InvalidArgument),
        );

        // Tests correct number of accounts are provided in deactivate
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &[],
                &serialize(&StakeInstruction::Deactivate).unwrap(),
                &mut MockInvokeContext::default()
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
