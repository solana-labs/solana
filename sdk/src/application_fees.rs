// This program is used to set an extra fee for a writable account
// The owner of an account or PDA can change the write account fee for the account,
// and this fee is only taken when you lock that account in write mode in a transaction.
// The fee will be charged even if eventually the transaction fails
// The fee will be charged only once per transaction even if there are multiple instructions locking the same account in write mode
// The owner of account will be reponsible to rebate the fees eventually or ( in the same instruction to not penalize good actors +roadmap )

#![cfg(feature = "full")]

use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
};
crate::declare_id!("App1icationFees1111111111111111111111111111");

#[derive(Serialize, Deserialize, Clone, Copy)]
pub struct ApplicationFeeStructure {
    pub fee_lamports: u64,
    pub version: u32,
    pub _padding : [u8;8],
}

pub const APPLICATION_FEE_STRUCTURE_SIZE : usize = 8 + 4 + 8;

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
    AddOrUpdateFee { fees: u64 },
    RemoveFees,
    // The write account owner i.e program usually can CPI this instruction to rebate the fees to good actors
    Rebate,
    // Rebase all fees beloning to the owner
    RebateAll,
}

impl ApplicationFeesInstuctions {
    pub fn add_or_update_fees(fees: u64, writable_account: Pubkey, owner: Pubkey, payer: Pubkey) -> Instruction {
        let (pda, _bump) = Pubkey::find_program_address(
            &[&writable_account.to_bytes()],
            &crate::application_fees::id(),
        );
        Instruction::new_with_borsh(
            id(),
            &Self::AddOrUpdateFee { fees: fees },
            vec![
                AccountMeta::new(writable_account, false),
                AccountMeta::new_readonly(owner, true),
                AccountMeta::new(pda, false),
                AccountMeta::new(payer, true),
            ],
        )
    }

    pub fn remove_fees(writable_account: Pubkey, owner: Pubkey) -> Instruction {
        let (pda, _bump) = Pubkey::find_program_address(
            &[&writable_account.to_bytes()],
            &crate::application_fees::id(),
        );
        Instruction::new_with_borsh(
            id(),
            &Self::RemoveFees,
            vec![
                AccountMeta::new(writable_account, false),
                AccountMeta::new_readonly(owner, true),
                AccountMeta::new(pda, false),
            ],
        )
    }

    pub fn rebate(writable_account: Pubkey, owner: Pubkey) -> Instruction {
        let (pda, _bump) = Pubkey::find_program_address(
            &[&writable_account.to_bytes()],
            &crate::application_fees::id(),
        );
        Instruction::new_with_borsh(
            id(),
            &Self::Rebate,
            vec![
                AccountMeta::new_readonly(writable_account, false),
                AccountMeta::new_readonly(owner, true),
                AccountMeta::new(pda, false),
            ],
        )
    }

    pub fn rebate_all(owner: Pubkey) -> Instruction {
        Instruction::new_with_borsh(
            id(),
            &Self::RebateAll,
            vec![AccountMeta::new_readonly(owner, true)],
        )
    }
}
