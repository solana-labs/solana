use {
    crate::{parse_instruction::parse_memo_data, VersionedTransactionWithStatusMeta},
    solana_sdk::{
        instruction::CompiledInstruction,
        message::{AccountKeys, Message, SanitizedMessage},
        pubkey::Pubkey,
    },
};

// A helper function to convert spl_memo::v1::id() as spl_sdk::pubkey::Pubkey to
// solana_sdk::pubkey::Pubkey
pub fn spl_memo_id_v1() -> Pubkey {
    *MEMO_PROGRAM_ID_V1
}

// A helper function to convert spl_memo::id() as spl_sdk::pubkey::Pubkey to
// solana_sdk::pubkey::Pubkey
pub fn spl_memo_id_v3() -> Pubkey {
    *MEMO_PROGRAM_ID_V3
}

lazy_static! {
    static ref MEMO_PROGRAM_ID_V1: Pubkey = Pubkey::new_from_array(spl_memo::v1::id().to_bytes());
    static ref MEMO_PROGRAM_ID_V3: Pubkey = Pubkey::new_from_array(spl_memo::id().to_bytes());
}

pub fn extract_and_fmt_memos<T: ExtractMemos>(message: &T) -> Option<String> {
    let memos = message.extract_memos();
    if memos.is_empty() {
        None
    } else {
        Some(memos.join("; "))
    }
}

fn extract_and_fmt_memo_data(data: &[u8]) -> String {
    let memo_len = data.len();
    let parsed_memo = parse_memo_data(data).unwrap_or_else(|_| "(unparseable)".to_string());
    format!("[{memo_len}] {parsed_memo}")
}

pub trait ExtractMemos {
    fn extract_memos(&self) -> Vec<String>;
}

impl ExtractMemos for Message {
    fn extract_memos(&self) -> Vec<String> {
        extract_memos_inner(
            &AccountKeys::new(&self.account_keys, None),
            &self.instructions,
        )
    }
}

impl ExtractMemos for SanitizedMessage {
    fn extract_memos(&self) -> Vec<String> {
        extract_memos_inner(&self.account_keys(), self.instructions())
    }
}

impl ExtractMemos for VersionedTransactionWithStatusMeta {
    fn extract_memos(&self) -> Vec<String> {
        extract_memos_inner(
            &self.account_keys(),
            self.transaction.message.instructions(),
        )
    }
}

enum KeyType<'a> {
    MemoProgram,
    OtherProgram,
    Unknown(&'a Pubkey),
}

fn extract_memos_inner(
    account_keys: &AccountKeys,
    instructions: &[CompiledInstruction],
) -> Vec<String> {
    let mut account_keys: Vec<KeyType> = account_keys.iter().map(KeyType::Unknown).collect();
    instructions
        .iter()
        .filter_map(|ix| {
            let index = ix.program_id_index as usize;
            let key_type = account_keys.get(index)?;
            let memo_data = match key_type {
                KeyType::MemoProgram => Some(&ix.data),
                KeyType::OtherProgram => None,
                KeyType::Unknown(program_id) => {
                    if **program_id == *MEMO_PROGRAM_ID_V1 || **program_id == *MEMO_PROGRAM_ID_V3 {
                        account_keys[index] = KeyType::MemoProgram;
                        Some(&ix.data)
                    } else {
                        account_keys[index] = KeyType::OtherProgram;
                        None
                    }
                }
            }?;
            Some(extract_and_fmt_memo_data(memo_data))
        })
        .collect()
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_extract_memos_inner() {
        let fee_payer = Pubkey::new_unique();
        let another_program_id = Pubkey::new_unique();
        let memo0 = "Test memo";
        let memo1 = "🦖";
        let expected_memos = vec![
            format!("[{}] {}", memo0.len(), memo0),
            format!("[{}] {}", memo1.len(), memo1),
        ];
        let memo_instructions = vec![
            CompiledInstruction {
                program_id_index: 1,
                accounts: vec![],
                data: memo0.as_bytes().to_vec(),
            },
            CompiledInstruction {
                program_id_index: 2,
                accounts: vec![],
                data: memo1.as_bytes().to_vec(),
            },
            CompiledInstruction {
                program_id_index: 3,
                accounts: vec![],
                data: memo1.as_bytes().to_vec(),
            },
        ];
        let static_keys = vec![
            fee_payer,
            spl_memo_id_v1(),
            another_program_id,
            spl_memo_id_v3(),
        ];
        let account_keys = AccountKeys::new(&static_keys, None);

        assert_eq!(
            extract_memos_inner(&account_keys, &memo_instructions),
            expected_memos
        );
    }
}
