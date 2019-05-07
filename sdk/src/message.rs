//! A library for generating a message from a sequence of instructions

use crate::hash::Hash;
use crate::instruction::{CompiledInstruction, Instruction};
use crate::pubkey::Pubkey;
use crate::short_vec;
use itertools::Itertools;

fn position(keys: &[Pubkey], key: &Pubkey) -> u8 {
    keys.iter().position(|k| k == key).unwrap() as u8
}

fn compile_instruction(
    ix: Instruction,
    keys: &[Pubkey],
    program_ids: &[Pubkey],
) -> CompiledInstruction {
    let accounts: Vec<_> = ix
        .accounts
        .iter()
        .map(|account_meta| position(keys, &account_meta.pubkey))
        .collect();

    CompiledInstruction {
        program_ids_index: position(program_ids, &ix.program_ids_index),
        data: ix.data.clone(),
        accounts,
    }
}

fn compile_instructions(
    ixs: Vec<Instruction>,
    keys: &[Pubkey],
    program_ids: &[Pubkey],
) -> Vec<CompiledInstruction> {
    ixs.into_iter()
        .map(|ix| compile_instruction(ix, keys, program_ids))
        .collect()
}

/// Return pubkeys referenced by all instructions, with the ones needing signatures first.
/// No duplicates and order is preserved.
fn get_keys(instructions: &[Instruction]) -> (Vec<Pubkey>, Vec<Pubkey>) {
    let mut keys_and_signed: Vec<_> = instructions
        .iter()
        .flat_map(|ix| ix.accounts.iter())
        .collect();
    keys_and_signed.sort_by(|x, y| y.is_signer.cmp(&x.is_signer));

    let mut signed_keys = vec![];
    let mut unsigned_keys = vec![];
    for account_meta in keys_and_signed.into_iter().unique_by(|x| x.pubkey) {
        if account_meta.is_signer {
            signed_keys.push(account_meta.pubkey);
        } else {
            unsigned_keys.push(account_meta.pubkey);
        }
    }
    (signed_keys, unsigned_keys)
}

/// Return program ids referenced by all instructions.  No duplicates and order is preserved.
fn get_program_ids(instructions: &[Instruction]) -> Vec<Pubkey> {
    instructions
        .iter()
        .map(|ix| ix.program_ids_index)
        .unique()
        .collect()
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct Message {
    /// The number of signatures required for this message to be considered valid. The
    /// signatures must match the first `num_required_signatures` of `account_keys`.
    pub num_required_signatures: u8,

    /// All the account keys used by this transaction
    #[serde(with = "short_vec")]
    pub account_keys: Vec<Pubkey>,

    /// The id of a recent ledger entry.
    pub recent_blockhash: Hash,

    /// All the program id keys used to execute this transaction's instructions
    #[serde(with = "short_vec")]
    program_ids: Vec<Pubkey>,

    /// Programs that will be executed in sequence and committed in one atomic transaction if all
    /// succeed.
    #[serde(with = "short_vec")]
    pub instructions: Vec<CompiledInstruction>,
}

impl Message {
    pub fn new_with_compiled_instructions(
        num_required_signatures: u8,
        account_keys: Vec<Pubkey>,
        recent_blockhash: Hash,
        program_ids: Vec<Pubkey>,
        instructions: Vec<CompiledInstruction>,
    ) -> Self {
        Self {
            num_required_signatures,
            account_keys,
            recent_blockhash,
            program_ids,
            instructions,
        }
    }

    pub fn new(instructions: Vec<Instruction>) -> Self {
        Self::new_with_payer(instructions, None)
    }

    pub fn new_with_payer(instructions: Vec<Instruction>, payer: Option<&Pubkey>) -> Self {
        let program_ids = get_program_ids(&instructions);
        let (mut signed_keys, unsigned_keys) = get_keys(&instructions);
        if let Some(payer) = payer {
            if signed_keys.is_empty() || signed_keys[0] != *payer {
                signed_keys.insert(0, *payer);
            }
        }
        let num_required_signatures = signed_keys.len() as u8;
        signed_keys.extend(&unsigned_keys);
        let instructions = compile_instructions(instructions, &signed_keys, &program_ids);
        Self::new_with_compiled_instructions(
            num_required_signatures,
            signed_keys,
            Hash::default(),
            program_ids,
            instructions,
        )
    }

    pub fn program_ids(&self) -> &[Pubkey] {
        &self.program_ids
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instruction::AccountMeta;
    use crate::signature::{Keypair, KeypairUtil};

    #[test]
    fn test_message_unique_program_ids() {
        let program_id0 = Pubkey::default();
        let program_ids = get_program_ids(&[
            Instruction::new(program_id0, &0, vec![]),
            Instruction::new(program_id0, &0, vec![]),
        ]);
        assert_eq!(program_ids, vec![program_id0]);
    }

    #[test]
    fn test_message_unique_program_ids_not_adjacent() {
        let program_id0 = Pubkey::default();
        let program_id1 = Pubkey::new_rand();
        let program_ids = get_program_ids(&[
            Instruction::new(program_id0, &0, vec![]),
            Instruction::new(program_id1, &0, vec![]),
            Instruction::new(program_id0, &0, vec![]),
        ]);
        assert_eq!(program_ids, vec![program_id0, program_id1]);
    }

    #[test]
    fn test_message_unique_program_ids_order_preserved() {
        let program_id0 = Pubkey::new_rand();
        let program_id1 = Pubkey::default(); // Key less than program_id0
        let program_ids = get_program_ids(&[
            Instruction::new(program_id0, &0, vec![]),
            Instruction::new(program_id1, &0, vec![]),
            Instruction::new(program_id0, &0, vec![]),
        ]);
        assert_eq!(program_ids, vec![program_id0, program_id1]);
    }

    #[test]
    fn test_message_unique_keys_both_signed() {
        let program_id = Pubkey::default();
        let id0 = Pubkey::default();
        let keys = get_keys(&[
            Instruction::new(program_id, &0, vec![AccountMeta::new(id0, true)]),
            Instruction::new(program_id, &0, vec![AccountMeta::new(id0, true)]),
        ]);
        assert_eq!(keys, (vec![id0], vec![]));
    }

    #[test]
    fn test_message_unique_keys_one_signed() {
        let program_id = Pubkey::default();
        let id0 = Pubkey::default();
        let keys = get_keys(&[
            Instruction::new(program_id, &0, vec![AccountMeta::new(id0, false)]),
            Instruction::new(program_id, &0, vec![AccountMeta::new(id0, true)]),
        ]);
        assert_eq!(keys, (vec![id0], vec![]));
    }

    #[test]
    fn test_message_unique_keys_order_preserved() {
        let program_id = Pubkey::default();
        let id0 = Pubkey::new_rand();
        let id1 = Pubkey::default(); // Key less than id0
        let keys = get_keys(&[
            Instruction::new(program_id, &0, vec![AccountMeta::new(id0, false)]),
            Instruction::new(program_id, &0, vec![AccountMeta::new(id1, false)]),
        ]);
        assert_eq!(keys, (vec![], vec![id0, id1]));
    }

    #[test]
    fn test_message_unique_keys_not_adjacent() {
        let program_id = Pubkey::default();
        let id0 = Pubkey::default();
        let id1 = Pubkey::new_rand();
        let keys = get_keys(&[
            Instruction::new(program_id, &0, vec![AccountMeta::new(id0, false)]),
            Instruction::new(program_id, &0, vec![AccountMeta::new(id1, false)]),
            Instruction::new(program_id, &0, vec![AccountMeta::new(id0, true)]),
        ]);
        assert_eq!(keys, (vec![id0], vec![id1]));
    }

    #[test]
    fn test_message_signed_keys_first() {
        let program_id = Pubkey::default();
        let id0 = Pubkey::default();
        let id1 = Pubkey::new_rand();
        let keys = get_keys(&[
            Instruction::new(program_id, &0, vec![AccountMeta::new(id0, false)]),
            Instruction::new(program_id, &0, vec![AccountMeta::new(id1, true)]),
        ]);
        assert_eq!(keys, (vec![id1], vec![id0]));
    }

    #[test]
    // Ensure there's a way to calculate the number of required signatures.
    fn test_message_signed_keys_len() {
        let program_id = Pubkey::default();
        let id0 = Pubkey::default();
        let ix = Instruction::new(program_id, &0, vec![AccountMeta::new(id0, false)]);
        let message = Message::new(vec![ix]);
        assert_eq!(message.num_required_signatures, 0);

        let ix = Instruction::new(program_id, &0, vec![AccountMeta::new(id0, true)]);
        let message = Message::new(vec![ix]);
        assert_eq!(message.num_required_signatures, 1);
    }

    #[test]
    fn test_message_kitchen_sink() {
        let program_id0 = Pubkey::default();
        let program_id1 = Pubkey::new_rand();
        let id0 = Pubkey::default();
        let keypair1 = Keypair::new();
        let id1 = keypair1.pubkey();
        let message = Message::new(vec![
            Instruction::new(program_id0, &0, vec![AccountMeta::new(id0, false)]),
            Instruction::new(program_id1, &0, vec![AccountMeta::new(id1, true)]),
            Instruction::new(program_id0, &0, vec![AccountMeta::new(id1, false)]),
        ]);
        assert_eq!(
            message.instructions[0],
            CompiledInstruction::new(0, &0, vec![1])
        );
        assert_eq!(
            message.instructions[1],
            CompiledInstruction::new(1, &0, vec![0])
        );
        assert_eq!(
            message.instructions[2],
            CompiledInstruction::new(0, &0, vec![0])
        );
    }

    #[test]
    fn test_message_payer_first() {
        let program_id = Pubkey::default();
        let payer = Pubkey::new_rand();
        let id0 = Pubkey::default();

        let ix = Instruction::new(program_id, &0, vec![AccountMeta::new(id0, false)]);
        let message = Message::new_with_payer(vec![ix], Some(&payer));
        assert_eq!(message.num_required_signatures, 1);

        let ix = Instruction::new(program_id, &0, vec![AccountMeta::new(id0, true)]);
        let message = Message::new_with_payer(vec![ix], Some(&payer));
        assert_eq!(message.num_required_signatures, 2);

        let ix = Instruction::new(
            program_id,
            &0,
            vec![AccountMeta::new(payer, true), AccountMeta::new(id0, true)],
        );
        let message = Message::new_with_payer(vec![ix], Some(&payer));
        assert_eq!(message.num_required_signatures, 2);
    }

}
