use {
    base64::{prelude::BASE64_STANDARD, Engine},
    serde::{
        de::{self, Deserialize, Deserializer},
        ser::{Serialize, SerializeSeq, Serializer},
    },
    solana_sdk::{
        account::{Account, AccountSharedData, ReadableAccount},
        clock::{Epoch, Slot},
        hash::Hash,
        pubkey::Pubkey,
    },
    std::str::FromStr,
};

#[derive(Deserialize, Serialize)]
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

// Wrap the Vec<...> so we can implement custom Serialize/Deserialize traits on the wrapper type
pub(crate) struct BankHashAccounts(pub Vec<(Pubkey, Hash, AccountSharedData)>);

#[derive(Deserialize, Serialize)]
/// Used as an intermediate for serializing and deserializing account fields
/// into a human readable format.
struct TempAccount {
    pubkey: String,
    hash: String,
    owner: String,
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
                owner: account.owner().to_string(),
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

impl<'de> Deserialize<'de> for BankHashAccounts {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let temp_accounts: Vec<TempAccount> = Deserialize::deserialize(deserializer)?;
        let pubkey_hash_accounts: Result<Vec<_>, _> = temp_accounts
            .into_iter()
            .map(|temp_account| {
                let pubkey = Pubkey::from_str(&temp_account.pubkey).map_err(de::Error::custom)?;
                let hash = Hash::from_str(&temp_account.hash).map_err(de::Error::custom)?;
                let account = AccountSharedData::from(Account {
                    lamports: temp_account.lamports,
                    data: BASE64_STANDARD
                        .decode(temp_account.data)
                        .map_err(de::Error::custom)?,
                    owner: Pubkey::from_str(&temp_account.owner).map_err(de::Error::custom)?,
                    executable: temp_account.executable,
                    rent_epoch: temp_account.rent_epoch,
                });
                Ok((pubkey, hash, account))
            })
            .collect();
        let pubkey_hash_accounts = pubkey_hash_accounts?;
        Ok(BankHashAccounts(pubkey_hash_accounts))
    }
}
