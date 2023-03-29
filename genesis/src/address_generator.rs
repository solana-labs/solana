use solana_sdk::pubkey::Pubkey;

#[derive(Default)]
pub struct AddressGenerator {
    base_pubkey: Pubkey,
    program_id: Pubkey,
    nth: usize,
}

impl AddressGenerator {
    pub fn new(base_pubkey: &Pubkey, program_id: &Pubkey) -> Self {
        Self {
            base_pubkey: *base_pubkey,
            program_id: *program_id,
            nth: 0,
        }
    }

    pub fn nth(&self, nth: usize) -> Pubkey {
        Pubkey::create_with_seed(&self.base_pubkey, &format!("{nth}"), &self.program_id).unwrap()
    }

    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Pubkey {
        let nth = self.nth;
        self.nth += 1;
        self.nth(nth)
    }
}
