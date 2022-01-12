use {
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount},
        pubkey::Pubkey,
    },
    std::ops::Deref,
};

#[cfg(RUSTC_WITH_SPECIALIZATION)]
impl ::solana_frozen_abi::abi_example::AbiExample for SysvarCache {
    fn example() -> Self {
        // SysvarCache is not Serialize so just rely on Default.
        SysvarCache::default()
    }
}

#[derive(Default, Clone, Debug)]
pub struct SysvarCache(Vec<(Pubkey, Vec<u8>)>);

impl Deref for SysvarCache {
    type Target = Vec<(Pubkey, Vec<u8>)>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl SysvarCache {
    pub fn push_entry(&mut self, pubkey: Pubkey, data: Vec<u8>) {
        self.0.push((pubkey, data));
    }

    pub fn update_entry(&mut self, pubkey: &Pubkey, new_account: &AccountSharedData) {
        if let Some(position) = self.iter().position(|(id, _data)| id == pubkey) {
            self.0[position].1 = new_account.data().to_vec();
        } else {
            self.0.push((*pubkey, new_account.data().to_vec()));
        }
    }
}
