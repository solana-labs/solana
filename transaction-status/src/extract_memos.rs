use {
    crate::parse_instruction::parse_memo_data,
    solana_sdk::{message::Message, pubkey::Pubkey},
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

pub fn extract_and_fmt_memos(message: &Message) -> Option<String> {
    let memos = extract_memos(message);
    if memos.is_empty() {
        None
    } else {
        Some(memos.join("; "))
    }
}

fn extract_memos(message: &Message) -> Vec<String> {
    let mut memos = vec![];
    if message.account_keys.contains(&spl_memo_id_v1())
        || message.account_keys.contains(&spl_memo_id_v3())
    {
        for instruction in &message.instructions {
            let program_id = message.account_keys[instruction.program_id_index as usize];
            if program_id == spl_memo_id_v1() || program_id == spl_memo_id_v3() {
                let memo_len = instruction.data.len();
                let parsed_memo = parse_memo_data(&instruction.data)
                    .unwrap_or_else(|_| "(unparseable)".to_string());
                memos.push(format!("[{}] {}", memo_len, parsed_memo));
            }
        }
    }
    memos
}
