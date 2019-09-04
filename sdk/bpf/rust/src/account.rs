use crate::pubkey::Pubkey;
use std::{cmp, fmt};

/// Account
pub struct Account<'a> {
    /// Public key of the account
    pub key: &'a Pubkey,
    /// Public key of the account
    pub is_signer: bool,
    /// Number of lamports owned by this account
    pub lamports: &'a mut u64,
    /// On-chain data within this account
    pub data: &'a mut [u8],
    /// Program that owns this account
    pub owner: &'a Pubkey,
    // TODO rent epoch?
}

impl<'a> fmt::Debug for Account<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let data_len = cmp::min(64, self.data.len());
        let data_str = if data_len > 0 {
            format!(" data: {}", hex::encode(self.data[..data_len].to_vec()))
        } else {
            "".to_string()
        };
        write!(
            f,
            "Account {{ lamports: {} data.len: {} owner: {} {} }}",
            self.lamports,
            self.data.len(),
            self.owner,
            // self.rent_epoch,
            data_str,
        )
    }
}
