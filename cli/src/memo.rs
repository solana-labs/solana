use solana_sdk::instruction::Instruction;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

pub trait WithMemo {
    fn with_memo(self, memo: Option<&String>) -> Self;
}

impl WithMemo for Vec<Instruction> {
    fn with_memo(mut self, memo: Option<&String>) -> Self {
        if let Some(memo) = &memo {
            let memo_ix = Instruction {
                program_id: Pubkey::from_str("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr")
                    .unwrap(),
                accounts: vec![],
                data: memo.as_bytes().to_vec(),
            };
            self.push(memo_ix);
        }
        self
    }
}
