use crate::id;
use crate::stake_state::{StakeAccount, StakeState};
use bincode::deserialize;
use log::*;
use serde_derive::{Deserialize, Serialize};
use solana_sdk::account::KeyedAccount;
use solana_sdk::instruction::{AccountMeta, Instruction, InstructionError};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::syscall;
use solana_sdk::system_instruction;

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum StakeInstruction {
    /// `Delegate` a stake to a particular node
    ///
    /// Expects 3 Accounts:
    ///    0 - Uninitialized StakeAccount to be delegated <= must have this signature
    ///    1 - VoteAccount to which this Stake will be delegated
    ///    2 - Current syscall Account that carries current bank epoch
    ///
    /// The u64 is the portion of the Stake account balance to be activated,
    ///    must be less than StakeAccount.lamports
    ///
    DelegateStake(u64),

    /// Redeem credits in the stake account
    ///
    /// Expects 4 Accounts:
    ///    0 - Delegate StakeAccount to be updated with rewards
    ///    1 - VoteAccount to which the Stake is delegated,
    ///    2 - RewardsPool Stake Account from which to redeem credits
    ///    3 - Rewards syscall Account that carries points values
    RedeemVoteCredits,

    /// Withdraw unstaked lamports from the stake account
    ///
    /// Expects 3 Accounts:
    ///    0 - Delegate StakeAccount
    ///    1 - System account to which the lamports will be transferred,
    ///    2 - Syscall Account that carries epoch
    ///
    /// The u64 is the portion of the Stake account balance to be withdrawn,
    ///    must be <= StakeAccount.lamports - staked lamports
    Withdraw(u64),

    /// Deactivates the stake in the account
    ///
    /// Expects 2 Accounts:
    ///    0 - Delegate StakeAccount
    ///    1 - Syscall Account that carries epoch
    Deactivate,
}

pub fn create_stake_account(
    from_pubkey: &Pubkey,
    staker_pubkey: &Pubkey,
    lamports: u64,
) -> Vec<Instruction> {
    vec![system_instruction::create_account(
        from_pubkey,
        staker_pubkey,
        lamports,
        std::mem::size_of::<StakeState>() as u64,
        &id(),
    )]
}

pub fn create_stake_account_and_delegate_stake(
    from_pubkey: &Pubkey,
    staker_pubkey: &Pubkey,
    vote_pubkey: &Pubkey,
    lamports: u64,
) -> Vec<Instruction> {
    let mut instructions = create_stake_account(from_pubkey, staker_pubkey, lamports);
    instructions.push(delegate_stake(staker_pubkey, vote_pubkey, lamports));
    instructions
}

pub fn redeem_vote_credits(stake_pubkey: &Pubkey, vote_pubkey: &Pubkey) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*stake_pubkey, false),
        AccountMeta::new(*vote_pubkey, false),
        AccountMeta::new(crate::rewards_pools::random_id(), false),
        AccountMeta::new_credit_only(syscall::rewards::id(), false),
    ];
    Instruction::new(id(), &StakeInstruction::RedeemVoteCredits, account_metas)
}

pub fn delegate_stake(stake_pubkey: &Pubkey, vote_pubkey: &Pubkey, stake: u64) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*stake_pubkey, true),
        AccountMeta::new(*vote_pubkey, false),
        AccountMeta::new_credit_only(syscall::current::id(), false),
    ];
    Instruction::new(id(), &StakeInstruction::DelegateStake(stake), account_metas)
}

pub fn withdraw(stake_pubkey: &Pubkey, to_pubkey: &Pubkey, lamports: u64) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*stake_pubkey, true),
        AccountMeta::new(*to_pubkey, false),
        AccountMeta::new_credit_only(syscall::current::id(), false),
    ];
    Instruction::new(id(), &StakeInstruction::Withdraw(lamports), account_metas)
}

pub fn deactivate_stake(stake_pubkey: &Pubkey) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*stake_pubkey, true),
        AccountMeta::new_credit_only(syscall::current::id(), false),
    ];
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

    if keyed_accounts.is_empty() {
        Err(InstructionError::InvalidInstructionData)?;
    }

    let (me, rest) = &mut keyed_accounts.split_at_mut(1);
    let me = &mut me[0];

    // TODO: data-driven unpack and dispatch of KeyedAccounts
    match deserialize(data).map_err(|_| InstructionError::InvalidInstructionData)? {
        StakeInstruction::DelegateStake(stake) => {
            if rest.len() != 2 {
                Err(InstructionError::InvalidInstructionData)?;
            }
            let vote = &rest[0];

            me.delegate_stake(
                vote,
                stake,
                &syscall::current::from_keyed_account(&rest[1])?,
            )
        }
        StakeInstruction::RedeemVoteCredits => {
            if rest.len() != 3 {
                Err(InstructionError::InvalidInstructionData)?;
            }
            let (vote, rest) = rest.split_at_mut(1);
            let vote = &mut vote[0];
            let (rewards_pool, rest) = rest.split_at_mut(1);
            let rewards_pool = &mut rewards_pool[0];

            me.redeem_vote_credits(
                vote,
                rewards_pool,
                &syscall::rewards::from_keyed_account(&rest[0])?,
            )
        }
        StakeInstruction::Withdraw(lamports) => {
            if rest.len() != 2 {
                Err(InstructionError::InvalidInstructionData)?;
            }
            let (to, syscall) = &mut rest.split_at_mut(1);
            let mut to = &mut to[0];

            me.withdraw(
                lamports,
                &mut to,
                &syscall::current::from_keyed_account(&syscall[0])?,
            )
        }
        StakeInstruction::Deactivate => {
            if rest.len() != 1 {
                Err(InstructionError::InvalidInstructionData)?;
            }
            let syscall = &rest[0];

            me.deactivate_stake(&syscall::current::from_keyed_account(&syscall)?)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bincode::serialize;
    use solana_sdk::account::Account;

    fn process_instruction(instruction: &Instruction) -> Result<(), InstructionError> {
        let mut accounts: Vec<_> = instruction
            .accounts
            .iter()
            .map(|meta| {
                if syscall::current::check_id(&meta.pubkey) {
                    syscall::current::create_account(1, 0, 0, 0)
                } else if syscall::rewards::check_id(&meta.pubkey) {
                    syscall::rewards::create_account(1, 0.0, 0.0)
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
            process_instruction(&redeem_vote_credits(&Pubkey::default(), &Pubkey::default(),)),
            Err(InstructionError::InvalidAccountData),
        );
        assert_eq!(
            process_instruction(&delegate_stake(&Pubkey::default(), &Pubkey::default(), 0)),
            Err(InstructionError::InvalidAccountData),
        );
        assert_eq!(
            process_instruction(&withdraw(&Pubkey::default(), &Pubkey::new_rand(), 100)),
            Err(InstructionError::InvalidAccountData),
        );
        assert_eq!(
            process_instruction(&deactivate_stake(&Pubkey::default())),
            Err(InstructionError::InvalidAccountData),
        );
    }

    #[test]
    fn test_stake_process_instruction_decode_bail() {
        // these will not call stake_state, have bogus contents

        // gets the first check
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &mut [KeyedAccount::new(
                    &Pubkey::default(),
                    false,
                    &mut Account::default(),
                )],
                &serialize(&StakeInstruction::DelegateStake(0)).unwrap(),
            ),
            Err(InstructionError::InvalidInstructionData),
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
                &serialize(&StakeInstruction::DelegateStake(0)).unwrap(),
            ),
            Err(InstructionError::InvalidInstructionData),
        );

        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &mut [
                    KeyedAccount::new(&Pubkey::default(), false, &mut Account::default()),
                    KeyedAccount::new(&Pubkey::default(), false, &mut Account::default()),
                ],
                &serialize(&StakeInstruction::RedeemVoteCredits).unwrap(),
            ),
            Err(InstructionError::InvalidInstructionData),
        );

        // gets the check non-deserialize-able account in delegate_stake
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &mut [
                    KeyedAccount::new(&Pubkey::default(), true, &mut Account::default()),
                    KeyedAccount::new(&Pubkey::default(), false, &mut Account::default()),
                    KeyedAccount::new(
                        &syscall::current::id(),
                        false,
                        &mut syscall::current::create_account(1, 0, 0, 0)
                    ),
                ],
                &serialize(&StakeInstruction::DelegateStake(0)).unwrap(),
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
                        &syscall::rewards::id(),
                        false,
                        &mut syscall::rewards::create_account(1, 0.0, 0.0)
                    ),
                ],
                &serialize(&StakeInstruction::RedeemVoteCredits).unwrap(),
            ),
            Err(InstructionError::InvalidAccountData),
        );

        // Tests 3rd keyed account is of correct type (Current instead of rewards) in withdraw
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &mut [
                    KeyedAccount::new(&Pubkey::default(), false, &mut Account::default()),
                    KeyedAccount::new(&Pubkey::default(), false, &mut Account::default()),
                    KeyedAccount::new(
                        &syscall::rewards::id(),
                        false,
                        &mut syscall::rewards::create_account(1, 0.0, 0.0)
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
                &mut [
                    KeyedAccount::new(&Pubkey::default(), false, &mut Account::default()),
                    KeyedAccount::new(
                        &syscall::current::id(),
                        false,
                        &mut syscall::rewards::create_account(1, 0.0, 0.0)
                    ),
                ],
                &serialize(&StakeInstruction::Withdraw(42)).unwrap(),
            ),
            Err(InstructionError::InvalidInstructionData),
        );

        // Tests 2nd keyed account is of correct type (Current instead of rewards) in deactivate
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &mut [
                    KeyedAccount::new(&Pubkey::default(), false, &mut Account::default()),
                    KeyedAccount::new(
                        &syscall::rewards::id(),
                        false,
                        &mut syscall::rewards::create_account(1, 0.0, 0.0)
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
                &mut [
                    KeyedAccount::new(&Pubkey::default(), false, &mut Account::default()),
                    KeyedAccount::new(&Pubkey::default(), false, &mut Account::default()),
                    KeyedAccount::new(
                        &syscall::current::id(),
                        false,
                        &mut syscall::rewards::create_account(1, 0.0, 0.0)
                    ),
                ],
                &serialize(&StakeInstruction::Deactivate).unwrap(),
            ),
            Err(InstructionError::InvalidInstructionData),
        );
    }

}
