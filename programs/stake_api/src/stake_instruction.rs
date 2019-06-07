use crate::id;
use crate::stake_state::{StakeAccount, StakeState};
use bincode::deserialize;
use log::*;
use serde_derive::{Deserialize, Serialize};
use solana_sdk::account::KeyedAccount;
use solana_sdk::instruction::{AccountMeta, Instruction, InstructionError};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::system_instruction;

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum StakeInstruction {
    /// Initialize the stake account as a Stake account.
    ///
    /// Expects 1 Accounts:
    ///    0 - StakeAccount to be initialized
    InitializeStake,

    // Initialize the stake account as a MiningPool account
    ///
    /// Expects 1 Accounts:
    ///    0 - MiningPool StakeAccount to be initialized
    InitializeMiningPool,

    /// `Delegate` a stake to a particular node
    ///
    /// Expects 2 Accounts:
    ///    0 - Delegate StakeAccount to be updated <= must have this signature
    ///    1 - VoteAccount to which this Stake will be delegated
    ///
    /// The u64 is the portion of the Stake account balance to be activated,
    ///    must be less than StakeAccount.lamports
    ///
    /// This instruction resets rewards, so issue
    DelegateStake(u64),

    /// Redeem credits in the stake account
    ///
    /// Expects 3 Accounts:
    ///    0 - MiningPool Stake Account to redeem credits from
    ///    1 - Delegate StakeAccount to be updated
    ///    2 - VoteAccount to which the Stake is delegated,
    RedeemVoteCredits,
}

pub fn create_stake_account(
    from_pubkey: &Pubkey,
    staker_pubkey: &Pubkey,
    lamports: u64,
) -> Vec<Instruction> {
    vec![
        system_instruction::create_account(
            from_pubkey,
            staker_pubkey,
            lamports,
            std::mem::size_of::<StakeState>() as u64,
            &id(),
        ),
        Instruction::new(
            id(),
            &StakeInstruction::InitializeStake,
            vec![AccountMeta::new(*staker_pubkey, false)],
        ),
    ]
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

pub fn create_mining_pool_account(
    from_pubkey: &Pubkey,
    staker_pubkey: &Pubkey,
    lamports: u64,
) -> Vec<Instruction> {
    vec![
        system_instruction::create_account(
            from_pubkey,
            staker_pubkey,
            lamports,
            std::mem::size_of::<StakeState>() as u64,
            &id(),
        ),
        Instruction::new(
            id(),
            &StakeInstruction::InitializeMiningPool,
            vec![AccountMeta::new(*staker_pubkey, false)],
        ),
    ]
}

pub fn redeem_vote_credits(
    mining_pool_pubkey: &Pubkey,
    stake_pubkey: &Pubkey,
    vote_pubkey: &Pubkey,
) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*mining_pool_pubkey, false),
        AccountMeta::new(*stake_pubkey, false),
        AccountMeta::new(*vote_pubkey, false),
    ];
    Instruction::new(id(), &StakeInstruction::RedeemVoteCredits, account_metas)
}

pub fn delegate_stake(stake_pubkey: &Pubkey, vote_pubkey: &Pubkey, stake: u64) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*stake_pubkey, true),
        AccountMeta::new(*vote_pubkey, false),
    ];
    Instruction::new(id(), &StakeInstruction::DelegateStake(stake), account_metas)
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
        StakeInstruction::InitializeMiningPool => {
            if !rest.is_empty() {
                Err(InstructionError::InvalidInstructionData)?;
            }
            me.initialize_mining_pool()
        }
        StakeInstruction::InitializeStake => {
            if !rest.is_empty() {
                Err(InstructionError::InvalidInstructionData)?;
            }
            me.initialize_stake()
        }
        StakeInstruction::DelegateStake(stake) => {
            if rest.len() != 1 {
                Err(InstructionError::InvalidInstructionData)?;
            }
            let vote = &rest[0];
            me.delegate_stake(vote, stake)
        }
        StakeInstruction::RedeemVoteCredits => {
            if rest.len() != 2 {
                Err(InstructionError::InvalidInstructionData)?;
            }
            let (stake, vote) = rest.split_at_mut(1);
            let stake = &mut stake[0];
            let vote = &mut vote[0];

            me.redeem_vote_credits(stake, vote)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bincode::serialize;
    use solana_sdk::account::Account;

    fn process_instruction(instruction: &Instruction) -> Result<(), InstructionError> {
        let mut accounts = vec![];
        for _ in 0..instruction.accounts.len() {
            accounts.push(Account::default());
        }
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
            process_instruction(&redeem_vote_credits(
                &Pubkey::default(),
                &Pubkey::default(),
                &Pubkey::default()
            )),
            Err(InstructionError::InvalidAccountData),
        );
        assert_eq!(
            process_instruction(&delegate_stake(&Pubkey::default(), &Pubkey::default(), 0)),
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

        // gets the check in delegate_stake
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &mut [
                    KeyedAccount::new(&Pubkey::default(), true, &mut Account::default()),
                    KeyedAccount::new(&Pubkey::default(), false, &mut Account::default()),
                ],
                &serialize(&StakeInstruction::DelegateStake(0)).unwrap(),
            ),
            Err(InstructionError::InvalidAccountData),
        );

        // gets the check in redeem_vote_credits
        assert_eq!(
            super::process_instruction(
                &Pubkey::default(),
                &mut [
                    KeyedAccount::new(&Pubkey::default(), false, &mut Account::default()),
                    KeyedAccount::new(&Pubkey::default(), false, &mut Account::default()),
                    KeyedAccount::new(&Pubkey::default(), false, &mut Account::default()),
                ],
                &serialize(&StakeInstruction::RedeemVoteCredits).unwrap(),
            ),
            Err(InstructionError::InvalidAccountData),
        );
    }

}
