use {crate::pubkey::Pubkey, solana_program::instruction::CompiledInstruction};

pub fn is_simple_vote_transaction<'a>(
    signatures_count: usize,
    instructions_count: usize,
    is_legacy_message: bool,
    mut instructions: impl Iterator<Item = (&'a Pubkey, &'a CompiledInstruction)>,
) -> bool {
    if signatures_count < 3 && instructions_count == 1 && is_legacy_message {
        instructions.next().map(|(program_id, _ix)| program_id) == Some(&crate::vote::program::id())
    } else {
        false
    }
}
