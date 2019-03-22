//! The `vote_script` module provides functionality for creating vote scripts.

use crate::id;
use crate::vote_instruction::VoteInstruction;
use crate::vote_state::VoteState;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::script::Script;
use solana_sdk::system_instruction::SystemInstruction;

pub struct VoteScript;

impl VoteScript {
    /// Fund or create the staking account with lamports
    pub fn new_account(from_id: &Pubkey, staker_id: &Pubkey, lamports: u64) -> Script {
        let space = VoteState::max_size() as u64;
        let create_ix =
            SystemInstruction::new_program_account(&from_id, staker_id, lamports, space, &id());
        let init_ix = VoteInstruction::new_initialize_account(staker_id);
        Script::new(vec![create_ix, init_ix])
    }
}
