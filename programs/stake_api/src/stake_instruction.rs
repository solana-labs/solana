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
    /// `Delegate` or `Assign` a stake account to a particular node
    ///  expects 2 KeyedAccounts:
    ///     StakeAccount to be updated
    ///     VoteAccount to which this Stake will be delegated
    DelegateStake,

    /// Redeem credits in the stake account
    ///  expects 3 KeyedAccounts: the StakeAccount to be updated
    ///  and the VoteAccount to which this Stake will be delegated
    RedeemVoteCredits,
}

pub fn create_account(from_id: &Pubkey, staker_id: &Pubkey, lamports: u64) -> Vec<Instruction> {
    vec![system_instruction::create_account(
        from_id,
        staker_id,
        lamports,
        std::mem::size_of::<StakeState>() as u64,
        &id(),
    )]
}

pub fn redeem_vote_credits(
    from_id: &Pubkey,
    mining_pool_id: &Pubkey,
    stake_id: &Pubkey,
    vote_id: &Pubkey,
) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*from_id, true),
        AccountMeta::new(*mining_pool_id, false),
        AccountMeta::new(*stake_id, false),
        AccountMeta::new(*vote_id, false),
    ];
    Instruction::new(id(), &StakeInstruction::RedeemVoteCredits, account_metas)
}

pub fn delegate_stake(from_id: &Pubkey, stake_id: &Pubkey, vote_id: &Pubkey) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*from_id, true),
        AccountMeta::new(*stake_id, true),
        AccountMeta::new(*vote_id, false),
    ];
    Instruction::new(id(), &StakeInstruction::DelegateStake, account_metas)
}

pub fn process_instruction(
    _program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    data: &[u8],
    _tick_height: u64,
) -> Result<(), InstructionError> {
    solana_logger::setup();

    trace!("process_instruction: {:?}", data);
    trace!("keyed_accounts: {:?}", keyed_accounts);

    if keyed_accounts.len() < 3 {
        Err(InstructionError::InvalidInstructionData)?;
    }

    // 0th index is the guy who paid for the transaction
    let (me, rest) = &mut keyed_accounts.split_at_mut(2);

    let me = &mut me[1];

    // TODO: data-driven unpack and dispatch of KeyedAccounts
    match deserialize(data).map_err(|_| InstructionError::InvalidInstructionData)? {
        StakeInstruction::DelegateStake => {
            if rest.len() != 1 {
                Err(InstructionError::InvalidInstructionData)?;
            }
            let vote = &rest[0];
            me.delegate_stake(vote)
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

    #[test]
    fn test_stake_process_instruction_decode_bail() {
        // these will not call stake_state, have bogus contents

        // gets the first check
        assert_eq!(
            process_instruction(
                &Pubkey::default(),
                &mut [KeyedAccount::new(
                    &Pubkey::default(),
                    false,
                    &mut Account::default(),
                )],
                &serialize(&StakeInstruction::DelegateStake).unwrap(),
                0,
            ),
            Err(InstructionError::InvalidInstructionData),
        );

        // gets the check in delegate_stake
        assert_eq!(
            process_instruction(
                &Pubkey::default(),
                &mut [
                    KeyedAccount::new(&Pubkey::default(), false, &mut Account::default()),
                    KeyedAccount::new(&Pubkey::default(), false, &mut Account::default()),
                ],
                &serialize(&StakeInstruction::DelegateStake).unwrap(),
                0,
            ),
            Err(InstructionError::InvalidInstructionData),
        );

        // gets the check in redeem_vote_credits
        assert_eq!(
            process_instruction(
                &Pubkey::default(),
                &mut [
                    KeyedAccount::new(&Pubkey::default(), false, &mut Account::default()),
                    KeyedAccount::new(&Pubkey::default(), false, &mut Account::default()),
                    KeyedAccount::new(&Pubkey::default(), false, &mut Account::default()),
                ],
                &serialize(&StakeInstruction::RedeemVoteCredits).unwrap(),
                0,
            ),
            Err(InstructionError::InvalidInstructionData),
        );
    }

}
