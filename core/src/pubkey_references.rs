use solana_sdk::pubkey::Pubkey;
use std::{collections::HashSet, rc::Rc};

#[derive(Default)]
pub struct PubkeyReferences(HashSet<Rc<Pubkey>>);

impl PubkeyReferences {
    pub fn get_or_insert(&mut self, pubkey: &Pubkey) -> Rc<Pubkey> {
        let mut cached_pubkey: Option<Rc<Pubkey>> = self.0.get(pubkey).cloned();
        if cached_pubkey.is_none() {
            let new_pubkey = Rc::new(*pubkey);
            self.0.insert(new_pubkey.clone());
            cached_pubkey = Some(new_pubkey);
        }
        cached_pubkey.unwrap()
    }

    pub fn purge(&mut self) {
        self.0.retain(|x| Rc::strong_count(x) > 1);
    }
}
