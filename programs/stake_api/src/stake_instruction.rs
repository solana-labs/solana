use crate::{
    config, id,
    stake_state::{Authorized, Lockup, StakeAccount, StakeAuthorize, StakeState},
};
use log::*;
use num_derive::{FromPrimitive, ToPrimitive};
use serde_derive::{Deserialize, Serialize};
use solana_sdk::{
    account::{get_signers, KeyedAccount},
    instruction::{AccountMeta, Instruction, InstructionError, WithSigner},
    instruction_processor_utils::{limited_deserialize, next_keyed_account, DecodeError},
    pubkey::Pubkey,
    system_instruction,
    sysvar::{
        self, clock::Clock, rent::Rent, rewards::Rewards, stake_history::StakeHistory, Sysvar,
    },
};

/// Reasons the stake might have had an error
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, FromPrimitive, ToPrimitive)]
pub enum StakeError {
    NoCreditsToRedeem,
    LockupInForce,
    AlreadyDeactivated,
    TooSoonToRedelegate,
    InsufficientStake,
}
impl<E> DecodeError<E> for StakeError {
    fn type_of() -> &'static str {
        "StakeError"
    }
}
impl std::fmt::Display for StakeError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            StakeError::NoCreditsToRedeem => write!(f, "not enough credits to redeem"),
            StakeError::LockupInForce => write!(f, "lockup has not yet expired"),
            StakeError::AlreadyDeactivated => write!(f, "stake already deactivated"),
            StakeError::TooSoonToRedelegate => write!(f, "one re-delegation permitted per epoch"),
            StakeError::InsufficientStake => write!(f, "split amount is more than is staked"),
        }
    }
}
impl std::error::Error for StakeError {}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub enum StakeInstruction {
    /// `Initialize` a stake with Lockup and Authorized information
    ///
    /// Expects 2 Accounts:
    ///    0 - Uninitialized StakeAccount
    ///    1 - Rent sysvar
    ///
    /// Authorized carries pubkeys that must sign staker transactions
    ///   and withdrawer transactions.
    /// Lockup carries information about withdrawal restrictions
    ///
    Initialize(Authorized, Lockup),

    /// Authorize a key to manage stake or withdrawal
    ///    requires Authorized::staker or Authorized::withdrawer
    ///    signature, depending on which key's being updated
    ///
    /// Expects 1 Account:
    ///    0 - StakeAccount to be updated with the Pubkey for
    ///          authorization
    Authorize(Pubkey, StakeAuthorize),

    /// `Delegate` a stake to a particular vote account
    ///    requires Authorized::staker signature
    ///
    /// Expects 4 Accounts:
    ///    0 - Initialized StakeAccount to be delegated
    ///    1 - VoteAccount to which this Stake will be delegated
    ///    2 - Clock sysvar Account that carries clock bank epoch
    ///    3 - Config Account that carries stake config
    ///
    /// The entire balance of the staking account is staked.  DelegateStake
    ///   can be called multiple times, but re-delegation is delayed
    ///   by one epoch
    ///
    DelegateStake,

    /// Redeem credits in the stake account
    ///    requires Authorized::staker signature
    ///
    /// Expects 5 Accounts:
    ///    0 - StakeAccount to be updated with rewards
    ///    1 - VoteAccount to which the Stake is delegated,
    ///    2 - RewardsPool Stake Account from which to redeem credits
    ///    3 - Rewards sysvar Account that carries points values
    ///    4 - StakeHistory sysvar that carries stake warmup/cooldown history
    ///
    RedeemVoteCredits,

    /// Split u64 tokens and stake off a stake account into another stake
    ///   account. Requires Authorized::staker signature.
    ///
    /// The split-off stake account must be Initialized and carry the
    ///   the same values for Lockup and Authorized as the source
    ///   or this instruction will fail.
    ///
    /// The source stake must be either Initialized or a Stake.
    ///
    /// Expects 2 Accounts:
    ///    0 - StakeAccount to be split
    ///    1 - Initialized StakeAcount that will take the split-off amount
    ///
    Split(u64),

    /// Withdraw unstaked lamports from the stake account
    ///    requires Authorized::withdrawer signature
    ///
    /// Expects 4 Accounts:
    ///    0 - StakeAccount from which to withdraw
    ///    1 - System account to which the lamports will be transferred,
    ///    2 - Syscall Account that carries epoch
    ///    3 - StakeHistory sysvar that carries stake warmup/cooldown history
    ///
    /// The u64 is the portion of the Stake account balance to be withdrawn,
    ///    must be <= StakeAccount.lamports - staked lamports.
    Withdraw(u64),

    /// Deactivates the stake in the account
    ///    requires Authorized::staker signature
    ///
    /// Expects 2 Accounts:
    ///    0 - Delegate StakeAccount
    ///    1 - Syscall Account that carries epoch
    ///
    Deactivate,
}

pub fn initialize(stake_pubkey: &Pubkey, authorized: &Authorized, lockup: &Lockup) -> Instruction {
    Instruction::new(
        id(),
        &StakeInstruction::Initialize(*authorized, *lockup),
        vec![
            AccountMeta::new(*stake_pubkey, false),
            AccountMeta::new_readonly(sysvar::rent::id(), false),
        ],
    )
}

pub fn create_stake_account_with_lockup(
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

pub fn split(
    stake_pubkey: &Pubkey,
    authorized_pubkey: &Pubkey,
    lamports: u64,
    split_stake_pubkey: &Pubkey,
) -> Vec<Instruction> {
    vec![
        system_instruction::create_account(
            stake_pubkey,
            split_stake_pubkey,
            0, // creates an ephemeral, uninitialized Stake
            std::mem::size_of::<StakeState>() as u64,
            &id(),
        ),
        {
            let account_metas = vec![
                AccountMeta::new(*stake_pubkey, false),
                AccountMeta::new(*split_stake_pubkey, false),
            ]
            .with_signer(authorized_pubkey);
            Instruction::new(id(), &StakeInstruction::Split(lamports), account_metas)
        },
    ]
}

pub fn create_stake_account(
    from_pubkey: &Pubkey,
    stake_pubkey: &Pubkey,
    authorized: &Authorized,
    lamports: u64,
) -> Vec<Instruction> {
    create_stake_account_with_lockup(
        from_pubkey,
        stake_pubkey,
        authorized,
        &Lockup::default(),
        lamports,
    )
}

pub fn create_stake_account_and_delegate_stake(
    from_pubkey: &Pubkey,
    stake_pubkey: &Pubkey,
    vote_pubkey: &Pubkey,
    authorized: &Authorized,
    lamports: u64,
) -> Vec<Instruction> {
    let mut instructions = create_stake_account(from_pubkey, stake_pubkey, authorized, lamports);
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
) -> Instruction {
    let account_metas = vec![AccountMeta::new(*stake_pubkey, false)].with_signer(authorized_pubkey);

    Instruction::new(
        id(),
        &StakeInstruction::Authorize(*new_authorized_pubkey, stake_authorize),
        account_metas,
    )
}

pub fn redeem_vote_credits(stake_pubkey: &Pubkey, vote_pubkey: &Pubkey) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*stake_pubkey, false),
        AccountMeta::new(*vote_pubkey, false),
        AccountMeta::new(crate::rewards_pools::random_id(), false),
        AccountMeta::new_readonly(sysvar::rewards::id(), false),
        AccountMeta::new_readonly(sysvar::stake_history::id(), false),
    ];
    Instruction::new(id(), &StakeInstruction::RedeemVoteCredits, account_metas)
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
        AccountMeta::new_readonly(crate::config::id(), false),
    ]
    .with_signer(authorized_pubkey);
    Instruction::new(id(), &StakeInstruction::DelegateStake, account_metas)
}

pub fn withdraw(
    stake_pubkey: &Pubkey,
    authorized_pubkey: &Pubkey,
    to_pubkey: &Pubkey,
    lamports: u64,
) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*stake_pubkey, false),
        AccountMeta::new(*to_pubkey, false),
        AccountMeta::new_readonly(sysvar::clock::id(), false),
        AccountMeta::new_readonly(sysvar::stake_history::id(), false),
    ]
    .with_signer(authorized_pubkey);
    Instruction::new(id(), &StakeInstruction::Withdraw(lamports), account_metas)
}

pub fn deactivate_stake(stake_pubkey: &Pubkey, authorized_pubkey: &Pubkey) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*stake_pubkey, false),
        AccountMeta::new_readonly(sysvar::clock::id(), false),
    ]
    .with_signer(authorized_pubkey);
    Instruction::new(id(), &StakeInstruction::Deactivate, account_metas)
}

pub fn process_instruction(
    _program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    data: &[u8],
) -> Result<(), InstructionError> {
    solana_logger::setup();

    trace!("process_instruction: {:?}", data);
    trace!("keyed_accounts: {:?}", keyed_accounts);

    let signers = get_signers(keyed_accounts);

    let keyed_accounts = &mut keyed_accounts.iter_mut();
    let me = &mut next_keyed_account(keyed_accounts)?;

    match limited_deserialize(data)? {
        StakeInstruction::Initialize(authorized, lockup) => me.initialize(
            &authorized,
            &lockup,
            &Rent::from_keyed_account(next_keyed_account(keyed_accounts)?)?,
        ),
        StakeInstruction::Authorize(authorized_pubkey, stake_authorize) => {
            me.authorize(&authorized_pubkey, stake_authorize, &signers)
        }
        StakeInstruction::DelegateStake => {
            let vote = next_keyed_account(keyed_accounts)?;

            me.delegate_stake(
                &vote,
                &Clock::from_keyed_account(next_keyed_account(keyed_accounts)?)?,
                &config::from_keyed_account(next_keyed_account(keyed_accounts)?)?,
                &signers,
            )
        }
        StakeInstruction::RedeemVoteCredits => {
            let vote = &mut next_keyed_account(keyed_accounts)?;
            let rewards_pool = &mut next_keyed_account(keyed_accounts)?;

            me.redeem_vote_credits(
                vote,
                rewards_pool,
                &Rewards::from_keyed_account(next_keyed_account(keyed_accounts)?)?,
                &StakeHistory::from_keyed_account(next_keyed_account(keyed_accounts)?)?,
            )
        }
        StakeInstruction::Split(lamports) => {
            let split_stake = &mut next_keyed_account(keyed_accounts)?;
            me.split(lamports, split_stake, &signers)
        }

        StakeInstruction::Withdraw(lamports) => {
            let to = &mut next_keyed_account(keyed_accounts)?;
            me.withdraw(
                lamports,
                to,
                &Clock::from_keyed_account(next_keyed_account(keyed_accounts)?)?,
                &StakeHistory::from_keyed_account(next_keyed_account(keyed_accounts)?)?,
                &signers,
            )
        }
        StakeInstruction::Deactivate => me.deactivate_stake(
            &Clock::from_keyed_account(next_keyed_account(keyed_accounts)?)?,
            &signers,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bincode::serialize;
    use solana_sdk::{account::Account, rent::Rent, sysvar::stake_history::StakeHistory};

    fn process_instruction(instruction: &Instruction) -> Result<(), InstructionError> {
        let mut accounts: Vec<_> = instruction
            .accounts
            .iter()
            .map(|meta| {
                if sysvar::clock::check_id(&meta.pubkey) {
                    sysvar::clock::create_account(1, 0, 0, 0, 0)
                } else if sysvar::rewards::check_id(&meta.pubkey) {
                    sysvar::rewards::create_account(1, 0.0, 0.0)
                } else if sysvar::stake_history::check_id(&meta.pubkey) {
                    sysvar::stake_history::create_account(1, &StakeHistory::default())
                } else if config::check_id(&meta.pubkey) {
                    config::create_account(1, &config::Config::default())
                } else if sysvar::rent::check_id(&meta.pubkey) {
                    sysvar::rent::create_account(1, &Rent::default())
                } else {
                    Account::default()
                }
            })
            .collect();

        {
            let mut keyed_accounts: Vec<_> = instruction
                .accounts
                .iter()
                .zip(accounts.iter_mut())
                .map(|(meta, account)| KeyedAccount::new(&meta.pubkey, meta.is_signer, account))
                .collect();
            super::process_instruction(&Pubkey::default(), &mut keyed_accounts, &instruction.data)
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
            process_instruction(&redeem_vote_credits(&Pubkey::default(), &Pubkey::default())),
            Err(InstructionError::InvalidAccountData),
        );
        assert_eq!(
            process_instruction(&authorize(
                &Pubkey::default(),
                &Pubkey::default(),
                &Pubkey::default(),
                StakeAuthorize::Staker
            )),
            Err(InstructionError::InvalidAccountData),
        );
        assert_eq!(
            process_instruction(
                &split(
                    &Pubkey::default(),
                    &Pubkey::default(),
                    100,
                    &Pubkey::default()
                )[1]
            ),
            Err(InstructionError::InvalidAccountData),
        );
        assert_eq!(
            process_instruction(&delegate_stake(
                &Pubkey::default(),
                &Pubkey::default(),
                &Pubkey::default()
            )),
            Err(InstructionError::InvalidAccountData),
        );
        assert_eq!(
            process_instruction(&withdraw(
                &Pubkey::default(),
                &Pubkey::default(),
                &Pubkey::new_rand(),
                100
            )),
            Err(InstructionError::InvalidAccountData),
        );
        assert_eq!(
            process_instruction(&deactivate_stake(&Pubkey::default(), &Pubkey::default())),
            Err(InstructionError::InvalidAccountData),
        );
    }

    #[test]
    fn test_stake_process_instruction_decode_bail() {
        // these will not call stake_state, have bogus contents

        // gets the "is_empty()" check
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &mut [],
                &serialize(&StakeInstruction::Initialize(
                    Authorized::default(),
                    Lockup::default()
                ))
                .unwrap(),
            ),
            Err(InstructionError::NotEnoughAccountKeys),
        );

        // no account for rent
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &mut [KeyedAccount::new(
                    &Pubkey::default(),
                    false,
                    &mut Account::default(),
                )],
                &serialize(&StakeInstruction::Initialize(
                    Authorized::default(),
                    Lockup::default()
                ))
                .unwrap(),
            ),
            Err(InstructionError::NotEnoughAccountKeys),
        );

        // rent fails to deserialize
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &mut [
                    KeyedAccount::new(&Pubkey::default(), false, &mut Account::default(),),
                    KeyedAccount::new(&sysvar::rent::id(), false, &mut Account::default(),)
                ],
                &serialize(&StakeInstruction::Initialize(
                    Authorized::default(),
                    Lockup::default()
                ))
                .unwrap(),
            ),
            Err(InstructionError::InvalidArgument),
        );

        // fails to deserialize stake state
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &mut [
                    KeyedAccount::new(&Pubkey::default(), false, &mut Account::default(),),
                    KeyedAccount::new(
                        &sysvar::rent::id(),
                        false,
                        &mut sysvar::rent::create_account(0, &Rent::default())
                    )
                ],
                &serialize(&StakeInstruction::Initialize(
                    Authorized::default(),
                    Lockup::default()
                ))
                .unwrap(),
            ),
            Err(InstructionError::InvalidAccountData),
        );

        // gets the first check in delegate, wrong number of accounts
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &mut [KeyedAccount::new(
                    &Pubkey::default(),
                    false,
                    &mut Account::default(),
                )],
                &serialize(&StakeInstruction::DelegateStake).unwrap(),
            ),
            Err(InstructionError::NotEnoughAccountKeys),
        );

        // gets the sub-check for number of args
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &mut [KeyedAccount::new(
                    &Pubkey::default(),
                    false,
                    &mut Account::default()
                ),],
                &serialize(&StakeInstruction::DelegateStake).unwrap(),
            ),
            Err(InstructionError::NotEnoughAccountKeys),
        );

        // catches the number of args check
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &mut [
                    KeyedAccount::new(&Pubkey::default(), false, &mut Account::default()),
                    KeyedAccount::new(&Pubkey::default(), false, &mut Account::default()),
                ],
                &serialize(&StakeInstruction::RedeemVoteCredits).unwrap(),
            ),
            Err(InstructionError::NotEnoughAccountKeys),
        );

        // catches the type of args check
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &mut [
                    KeyedAccount::new(&Pubkey::default(), false, &mut Account::default()),
                    KeyedAccount::new(&Pubkey::default(), false, &mut Account::default()),
                    KeyedAccount::new(&Pubkey::default(), false, &mut Account::default()),
                    KeyedAccount::new(&Pubkey::default(), false, &mut Account::default()),
                    KeyedAccount::new(&Pubkey::default(), false, &mut Account::default()),
                ],
                &serialize(&StakeInstruction::RedeemVoteCredits).unwrap(),
            ),
            Err(InstructionError::InvalidArgument),
        );

        // gets the check non-deserialize-able account in delegate_stake
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &mut [
                    KeyedAccount::new(&Pubkey::default(), true, &mut Account::default()),
                    KeyedAccount::new(&Pubkey::default(), false, &mut Account::default()),
                    KeyedAccount::new(
                        &sysvar::clock::id(),
                        false,
                        &mut sysvar::clock::create_account(1, 0, 0, 0, 0)
                    ),
                    KeyedAccount::new(
                        &config::id(),
                        false,
                        &mut config::create_account(1, &config::Config::default())
                    ),
                ],
                &serialize(&StakeInstruction::DelegateStake).unwrap(),
            ),
            Err(InstructionError::InvalidAccountData),
        );

        // gets the deserialization checks in redeem_vote_credits
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &mut [
                    KeyedAccount::new(&Pubkey::default(), false, &mut Account::default()),
                    KeyedAccount::new(&Pubkey::default(), false, &mut Account::default()),
                    KeyedAccount::new(&Pubkey::default(), false, &mut Account::default()),
                    KeyedAccount::new(
                        &sysvar::rewards::id(),
                        false,
                        &mut sysvar::rewards::create_account(1, 0.0, 0.0)
                    ),
                    KeyedAccount::new(
                        &sysvar::stake_history::id(),
                        false,
                        &mut sysvar::stake_history::create_account(1, &StakeHistory::default())
                    ),
                ],
                &serialize(&StakeInstruction::RedeemVoteCredits).unwrap(),
            ),
            Err(InstructionError::InvalidAccountData),
        );

        // Tests 3rd keyed account is of correct type (Clock instead of rewards) in withdraw
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &mut [
                    KeyedAccount::new(&Pubkey::default(), false, &mut Account::default()),
                    KeyedAccount::new(&Pubkey::default(), false, &mut Account::default()),
                    KeyedAccount::new(
                        &sysvar::rewards::id(),
                        false,
                        &mut sysvar::rewards::create_account(1, 0.0, 0.0)
                    ),
                    KeyedAccount::new(
                        &sysvar::stake_history::id(),
                        false,
                        &mut sysvar::stake_history::create_account(1, &StakeHistory::default())
                    ),
                ],
                &serialize(&StakeInstruction::Withdraw(42)).unwrap(),
            ),
            Err(InstructionError::InvalidArgument),
        );

        // Tests correct number of accounts are provided in withdraw
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &mut [KeyedAccount::new(
                    &Pubkey::default(),
                    false,
                    &mut Account::default()
                )],
                &serialize(&StakeInstruction::Withdraw(42)).unwrap(),
            ),
            Err(InstructionError::NotEnoughAccountKeys),
        );

        // Tests 2nd keyed account is of correct type (Clock instead of rewards) in deactivate
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &mut [
                    KeyedAccount::new(&Pubkey::default(), false, &mut Account::default()),
                    KeyedAccount::new(
                        &sysvar::rewards::id(),
                        false,
                        &mut sysvar::rewards::create_account(1, 0.0, 0.0)
                    ),
                ],
                &serialize(&StakeInstruction::Deactivate).unwrap(),
            ),
            Err(InstructionError::InvalidArgument),
        );

        // Tests correct number of accounts are provided in deactivate
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &mut [],
                &serialize(&StakeInstruction::Deactivate).unwrap(),
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
            if let InstructionError::CustomError(code) = err {
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
            "CustomError(0): StakeError::NoCreditsToRedeem - not enough credits to redeem",
            pretty_err::<StakeError>(StakeError::NoCreditsToRedeem.into())
        )
    }
}
