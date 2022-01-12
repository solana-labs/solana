use {
    super::Bank,
    solana_sdk::{
        account::ReadableAccount,
        pubkey::Pubkey,
        sysvar::{self, Sysvar},
    },
};

impl Bank {
    pub(crate) fn fill_sysvar_cache(&mut self) {
        let mut sysvar_cache = self.sysvar_cache.write().unwrap();
        for id in sysvar::ALL_IDS.iter() {
            if !sysvar_cache.iter().any(|(key, _data)| key == id) {
                if let Some(account) = self.get_account_with_fixed_root(id) {
                    sysvar_cache.push_entry(*id, account.data().to_vec());
                }
            }
        }
    }

    /// Get the value of a cached sysvar by its id
    pub fn get_cached_sysvar<T: Sysvar>(&self, id: &Pubkey) -> Option<T> {
        let sysvar_cache = self.sysvar_cache.read().unwrap();
        sysvar_cache.iter().find_map(|(key, data)| {
            if id == key {
                bincode::deserialize(data).ok()
            } else {
                None
            }
        })
    }
}
