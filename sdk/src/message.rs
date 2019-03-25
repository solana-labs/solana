use crate::hash::Hash;
use crate::instruction::CompiledInstruction;
use crate::pubkey::Pubkey;

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Message {
    pub num_signatures: u8,
    pub account_keys: Vec<Pubkey>,
    pub recent_blockhash: Hash,
    pub fee: u64,
    pub program_ids: Vec<Pubkey>,
    pub instructions: Vec<CompiledInstruction>,
}
