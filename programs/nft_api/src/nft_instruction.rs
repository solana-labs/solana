use crate::nft_state::NftState;
use bincode::serialized_size;
use num_derive::{FromPrimitive, ToPrimitive};
use serde_derive::{Deserialize, Serialize};
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    instruction_processor_utils::DecodeError,
    pubkey::Pubkey,
    short_vec, system_instruction,
};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, FromPrimitive, ToPrimitive)]
pub enum NftError {
    IncorrectOwner,
}

impl<T> DecodeError<T> for NftError {
    fn type_of() -> &'static str {
        "NftError"
    }
}

impl std::fmt::Display for NftError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                NftError::IncorrectOwner => "incorrect owner",
            }
        )
    }
}
impl std::error::Error for NftError {}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct InitializeAccountParams {
    pub issuer_pubkey: Pubkey,
    pub owner_pubkey: Pubkey,
    #[serde(with = "short_vec")]
    pub id: Vec<u8>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum NftInstruction {
    InitializeAccount(InitializeAccountParams),
    SetOwner(Pubkey),
}

fn initialize_account(
    contract_pubkey: &Pubkey,
    issuer_pubkey: &Pubkey,
    owner_pubkey: &Pubkey,
    id: Vec<u8>,
) -> Instruction {
    let params = InitializeAccountParams {
        owner_pubkey: *owner_pubkey,
        issuer_pubkey: *issuer_pubkey,
        id,
    };
    let keys = vec![
        AccountMeta::new(*contract_pubkey, false),
        AccountMeta::new(*issuer_pubkey, true),
    ];
    Instruction::new(
        crate::id(),
        &NftInstruction::InitializeAccount(params),
        keys,
    )
}

pub fn create_account(
    payer_pubkey: &Pubkey,
    contract_pubkey: &Pubkey,
    issuer_pubkey: &Pubkey,
    owner_pubkey: &Pubkey,
    id: Vec<u8>,
    lamports: u64,
) -> Vec<Instruction> {
    let claim_state = NftState {
        id: id.clone(),
        ..NftState::default()
    };
    let space = serialized_size(&claim_state).unwrap();
    vec![
        system_instruction::create_account(
            &payer_pubkey,
            contract_pubkey,
            lamports,
            space,
            &crate::id(),
        ),
        initialize_account(contract_pubkey, issuer_pubkey, owner_pubkey, id),
    ]
}

pub fn set_owner(
    contract_pubkey: &Pubkey,
    old_pubkey: &Pubkey,
    new_pubkey: &Pubkey,
) -> Instruction {
    let keys = vec![
        AccountMeta::new(*contract_pubkey, false),
        AccountMeta::new(*old_pubkey, true),
    ];
    Instruction::new(crate::id(), &NftInstruction::SetOwner(*new_pubkey), keys)
}
