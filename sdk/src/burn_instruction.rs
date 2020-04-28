use crate::{
    burn_program,
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
};

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum BurnInstruction {
    /// Burn an account
    ///
    /// * key[0] - the account to burn
    ///
    /// The transaction must be signed by key[0]
    Burn,
}

pub fn burn(account_pubkey: &Pubkey) -> Instruction {
    let account_metas = vec![AccountMeta::new(*account_pubkey, true)];
    Instruction::new(
        burn_program::id(),
        &BurnInstruction::Burn,
        account_metas,
    )
}
