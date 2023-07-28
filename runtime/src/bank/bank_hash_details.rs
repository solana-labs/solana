use {
    base64::{prelude::BASE64_STANDARD, Engine},
    serde::ser::{Serialize, SerializeSeq, Serializer},
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount},
        clock::{Epoch, Slot},
        hash::Hash,
        pubkey::Pubkey,
    },
};

#[derive(Serialize)]
pub(crate) struct BankHashDetails {
    /// client version
    pub version: String,
    pub slot: Slot,
    pub bank_hash: String,
    pub parent_bank_hash: String,
    pub accounts_delta_hash: String,
    pub signature_count: u64,
    pub last_blockhash: String,
    pub accounts: BankHashAccounts,
}

pub(crate) struct BankHashAccounts(pub Vec<(Pubkey, Hash, AccountSharedData)>);

#[derive(Serialize)]
struct TempAccount {
    pubkey: String,
    hash: String,
    lamports: u64,
    rent_epoch: Epoch,
    executable: bool,
    data: String,
}

impl Serialize for BankHashAccounts {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
        for (pubkey, hash, account) in self.0.iter() {
            let temp = TempAccount {
                pubkey: pubkey.to_string(),
                hash: hash.to_string(),
                lamports: account.lamports(),
                rent_epoch: account.rent_epoch(),
                executable: account.executable(),
                data: BASE64_STANDARD.encode(account.data()),
            };
            seq.serialize_element(&temp)?;
        }
        seq.end()
    }
}
