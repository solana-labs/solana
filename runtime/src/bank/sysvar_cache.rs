use {
    super::Bank,
    solana_sdk::{account::ReadableAccount, sysvar},
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
}
