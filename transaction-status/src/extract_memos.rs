use {
    crate::parse_instruction::parse_memo_data,
    solana_sdk::{
        message::{Message, SanitizedMessage},
        pubkey::Pubkey,
    },
};

// A helper function to convert spl_memo::v1::id() as spl_sdk::pubkey::Pubkey to
// solana_sdk::pubkey::Pubkey
pub fn spl_memo_id_v1() -> Pubkey {
    Pubkey::new_from_array(spl_memo::v1::id().to_bytes())
}

// A helper function to convert spl_memo::id() as spl_sdk::pubkey::Pubkey to
// solana_sdk::pubkey::Pubkey
pub fn spl_memo_id_v3() -> Pubkey {
    Pubkey::new_from_array(spl_memo::id().to_bytes())
}

pub fn extract_and_fmt_memos<T: ExtractMemos>(message: &T) -> Option<String> {
    let memos = message.extract_memos();
    if memos.is_empty() {
        None
    } else {
        Some(memos.join("; "))
    }
}

fn maybe_push_parsed_memo(memos: &mut Vec<String>, program_id: Pubkey, data: &[u8]) {
    if program_id == spl_memo_id_v1() || program_id == spl_memo_id_v3() {
        let memo_len = data.len();
        let parsed_memo = parse_memo_data(data).unwrap_or_else(|_| "(unparseable)".to_string());
        memos.push(format!("[{}] {}", memo_len, parsed_memo));
    }
}

pub trait ExtractMemos {
    fn extract_memos(&self) -> Vec<String>;
}

impl ExtractMemos for Message {
    fn extract_memos(&self) -> Vec<String> {
        let mut memos = vec![];
        if self.account_keys.contains(&spl_memo_id_v1())
            || self.account_keys.contains(&spl_memo_id_v3())
        {
            for instruction in &self.instructions {
                let program_id = self.account_keys[instruction.program_id_index as usize];
                maybe_push_parsed_memo(&mut memos, program_id, &instruction.data);
            }
        }
        memos
    }
}

impl ExtractMemos for SanitizedMessage {
    fn extract_memos(&self) -> Vec<String> {
        let mut memos = vec![];
        if self
            .account_keys_iter()
            .any(|&pubkey| pubkey == spl_memo_id_v1() || pubkey == spl_memo_id_v3())
        {
            for (program_id, instruction) in self.program_instructions_iter() {
                maybe_push_parsed_memo(&mut memos, *program_id, &instruction.data);
            }
        }
        memos
    }
}

#[cfg(test)]
mod test {
    use {
        super::*,
        solana_sdk::{
            hash::Hash,
            instruction::CompiledInstruction,
            message::{
                v0::{self, LoadedAddresses},
                MessageHeader,
            },
        },
    };

    #[test]
    fn test_extract_memos() {
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
        let message = Message::new_with_compiled_instructions(
            1,
            0,
            3,
            vec![
                fee_payer,
                spl_memo_id_v1(),
                another_program_id,
                spl_memo_id_v3(),
            ],
            Hash::default(),
            memo_instructions.clone(),
        );
        assert_eq!(message.extract_memos(), expected_memos);

        let sanitized_message = SanitizedMessage::Legacy(message);
        assert_eq!(sanitized_message.extract_memos(), expected_memos);

        let sanitized_message = SanitizedMessage::V0(v0::LoadedMessage {
            message: v0::Message {
                header: MessageHeader {
                    num_required_signatures: 1,
                    num_readonly_signed_accounts: 0,
                    num_readonly_unsigned_accounts: 3,
                },
                account_keys: vec![fee_payer],
                instructions: memo_instructions,
                ..v0::Message::default()
            },
            loaded_addresses: LoadedAddresses {
                writable: vec![],
                readonly: vec![spl_memo_id_v1(), another_program_id, spl_memo_id_v3()],
            },
        });
        assert_eq!(sanitized_message.extract_memos(), expected_memos);
    }
}
