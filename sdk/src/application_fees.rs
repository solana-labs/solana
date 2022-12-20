// This program is used to set an extra fee for a writable account
// The owner of an account or PDA can change the write account fee for the account, 
// and this fee is only taken when you lock that account in write mode in a transaction.
// The fee will be charged even if eventually the transaction fails
// The fee will be charged only once per transaction even if there are multiple instructions locking the same account in write mode
// The owner of account will be reponsible to rebate the fees eventually or ( in the same instruction to not penalize good actors +roadmap )

#![cfg(feature = "full")]

use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::{instruction::Instruction};
crate::declare_id!("App1icationFees1111111111111111111111111111");

#[derive(Serialize, Deserialize, Clone, Copy)]
pub struct ApplicationFeeStructure {
    pub fee_lamports: u64,
    pub version: u32,
}

// application fees instructions
#[derive(
    AbiExample,
    AbiEnumVisitor,
    BorshDeserialize,
    BorshSerialize,
    Clone,
    Debug,
    Deserialize,
    PartialEq,
    Eq,
    Serialize,
)]
pub enum ApplicationFeesInstuctions {
    // Add fee to a account
    AddOrUpdateFee {
        fees : u64,
    },
    RemoveFees,
}

impl ApplicationFeesInstuctions {
    pub fn add_or_update_fees(fees: u64) -> Instruction {
        Instruction::new_with_borsh(id(), &Self::AddOrUpdateFee { fees: fees}, vec![])
    }

    pub fn remove_fees() -> Instruction {
        Instruction::new_with_borsh(id(), &Self::RemoveFees, vec![])
    }
}