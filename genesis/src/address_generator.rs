use solana_sdk::{hash::hashv, pubkey::Pubkey};

#[derive(Default)]
pub struct AddressGenerator {
    base_pubkey: Pubkey,
    base_name: String,
    nth: usize,
}

impl AddressGenerator {
    pub fn new(base_pubkey: &Pubkey, base_name: &str) -> Self {
        Self {
            base_pubkey: *base_pubkey,
            base_name: base_name.to_string(),
            nth: 0,
        }
    }
    pub fn nth(&self, nth: usize) -> Pubkey {
        Pubkey::new(
            hashv(&[
                self.base_pubkey.as_ref(),
                format!("{}:{}", self.base_name, nth).as_bytes(),
            ])
            .as_ref(),
        )
    }
    pub fn next(&mut self) -> Pubkey {
        let nth = self.nth;
        self.nth += 1;
        self.nth(nth)
    }
}
