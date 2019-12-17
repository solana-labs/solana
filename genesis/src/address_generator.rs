use solana_sdk::{pubkey::Pubkey, system_instruction::create_address_with_seed};

#[derive(Default)]
pub struct AddressGenerator {
    base_pubkey: Pubkey,
    base_seed: String,
    program_id: Pubkey,
    nth: usize,
}

impl AddressGenerator {
    pub fn new(base_pubkey: &Pubkey, base_seed: &str, program_id: &Pubkey) -> Self {
        Self {
            base_pubkey: *base_pubkey,
            base_seed: base_seed.to_string(),
            program_id: *program_id,
            nth: 0,
        }
    }

    pub fn nth(&self, nth: usize) -> Pubkey {
        create_address_with_seed(
            &self.base_pubkey,
            &format!("{}:{}", self.base_seed, nth),
            &self.program_id,
        )
        .unwrap()
    }

    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Pubkey {
        let nth = self.nth;
        self.nth += 1;
        self.nth(nth)
    }
}
