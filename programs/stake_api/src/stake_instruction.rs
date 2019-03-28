use crate::id;
use crate::stake_state::StakeState;
//use crate::stake_state::StakeAccount;
use bincode::deserialize;
use log::*;
use serde_derive::{Deserialize, Serialize};
use solana_sdk::account::KeyedAccount;
use solana_sdk::instruction::{AccountMeta, Instruction};
use solana_sdk::instruction_error::InstructionError;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::system_instruction::SystemInstruction;

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

impl StakeInstruction {
    pub fn new_account(from_id: &Pubkey, staker_id: &Pubkey, lamports: u64) -> Vec<Instruction> {
        vec![SystemInstruction::new_program_account(
            &from_id,
            staker_id,
            lamports,
            std::mem::size_of::<StakeState>() as u64,
            &id(),
        )]
    }

    pub fn new_redeem_vote_credits(
        from_id: &Pubkey,
        stake_id: &Pubkey,
        vote_id: &Pubkey,
    ) -> Instruction {
        let account_metas = vec![
            AccountMeta::new(*from_id, false),
            AccountMeta::new(*stake_id, false),
            AccountMeta::new(*vote_id, false),
        ];
        Instruction::new(id(), &StakeInstruction::RedeemVoteCredits, account_metas)
    }

    pub fn new_delegate_stake(stake_id: &Pubkey, vote_id: &Pubkey) -> Instruction {
        let account_metas = vec![
            AccountMeta::new(*stake_id, true),
            AccountMeta::new(*vote_id, false),
        ];
        Instruction::new(id(), &StakeInstruction::DelegateStake, account_metas)
    }
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

    //    let me = &mut keyed_accounts[0];
    //
    match deserialize(data).map_err(|_| InstructionError::InvalidInstructionData)? {
        //        StakeInstruction::DelegateStake => me.delegate_stake(&keyed_accounts[1]),
        //        StakeInstruction::RedeemVoteCredits => {
        //            me.redeem_vote_credits(&mut keyed_accounts[1], &keyed_accounts[2])
        //        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {}
