use num_derive::{FromPrimitive, ToPrimitive};
use serde_derive::{Deserialize, Serialize};
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    instruction_processor_utils::DecodeError,
    pubkey::Pubkey,
    system_instruction,
};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, FromPrimitive, ToPrimitive)]
pub enum OwnableError {
    IncorrectOwner,
}

impl<T> DecodeError<T> for OwnableError {
    fn type_of() -> &'static str {
        "OwnableError"
    }
}

impl std::fmt::Display for OwnableError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                OwnableError::IncorrectOwner => "incorrect owner",
            }
        )
    }
}
impl std::error::Error for OwnableError {}

fn initialize_account(account_pubkey: &Pubkey, owner_pubkey: &Pubkey) -> Instruction {
    let keys = vec![AccountMeta::new(*account_pubkey, false)];
    Instruction::new(crate::id(), &owner_pubkey, keys)
}

pub fn create_account(
    payer_pubkey: &Pubkey,
    account_pubkey: &Pubkey,
    owner_pubkey: &Pubkey,
    lamports: u64,
) -> Vec<Instruction> {
    let space = std::mem::size_of::<Pubkey>() as u64;
    vec![
        system_instruction::create_account(
            &payer_pubkey,
            account_pubkey,
            lamports,
            space,
            &crate::id(),
        ),
        initialize_account(account_pubkey, owner_pubkey),
    ]
}

pub fn set_owner(account_pubkey: &Pubkey, old_pubkey: &Pubkey, new_pubkey: &Pubkey) -> Instruction {
    let keys = vec![
        AccountMeta::new(*account_pubkey, false),
        AccountMeta::new(*old_pubkey, true),
    ];
    Instruction::new(crate::id(), &new_pubkey, keys)
}
