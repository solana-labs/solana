//! Container to capture information relevant to computing a bank hash

use {
    super::Bank,
    base64::{prelude::BASE64_STANDARD, Engine},
    log::*,
    serde::{
        de::{self, Deserialize, Deserializer},
        ser::{Serialize, SerializeSeq, Serializer},
    },
    solana_accounts_db::{accounts_db::PubkeyHashAccount, accounts_hash::AccountsDeltaHash},
    solana_sdk::{
        account::{Account, AccountSharedData, ReadableAccount},
        clock::{Epoch, Slot},
        hash::Hash,
        pubkey::Pubkey,
    },
    std::str::FromStr,
};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct BankHashDetails {
    /// client version
    pub version: String,
    pub account_data_encoding: String,
    pub slot: Slot,
    pub bank_hash: String,
    pub parent_bank_hash: String,
    pub accounts_delta_hash: String,
    pub signature_count: u64,
    pub last_blockhash: String,
    pub accounts: BankHashAccounts,
}

impl BankHashDetails {
    pub fn new(
        slot: Slot,
        bank_hash: Hash,
        parent_bank_hash: Hash,
        accounts_delta_hash: Hash,
        signature_count: u64,
        last_blockhash: Hash,
        accounts: BankHashAccounts,
    ) -> Self {
        Self {
            version: solana_version::version!().to_string(),
            account_data_encoding: "base64".to_string(),
            slot,
            bank_hash: bank_hash.to_string(),
            parent_bank_hash: parent_bank_hash.to_string(),
            accounts_delta_hash: accounts_delta_hash.to_string(),
            signature_count,
            last_blockhash: last_blockhash.to_string(),
            accounts,
        }
    }
}

impl TryFrom<&Bank> for BankHashDetails {
    type Error = String;

    fn try_from(bank: &Bank) -> Result<Self, Self::Error> {
        let slot = bank.slot();
        if !bank.is_frozen() {
            return Err(format!(
                "Bank {slot} must be frozen in order to get bank hash details"
            ));
        }

        // This bank is frozen; as a result, we know that the state has been
        // hashed which means the delta hash is Some(). So, .unwrap() is safe
        let AccountsDeltaHash(accounts_delta_hash) = bank
            .rc
            .accounts
            .accounts_db
            .get_accounts_delta_hash(slot)
            .unwrap();
        let mut accounts = bank
            .rc
            .accounts
            .accounts_db
            .get_pubkey_hash_account_for_slot(slot);
        // get_pubkey_hash_account_for_slot() returns an arbitrary ordering;
        // sort by pubkey to match the ordering used for accounts delta hash
        accounts.sort_by_key(|account| account.pubkey);

        Ok(Self::new(
            slot,
            bank.hash(),
            bank.parent_hash(),
            accounts_delta_hash,
            bank.signature_count(),
            bank.last_blockhash(),
            BankHashAccounts { accounts },
        ))
    }
}

// Wrap the Vec<...> so we can implement custom Serialize/Deserialize traits on the wrapper type
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BankHashAccounts {
    pub accounts: Vec<PubkeyHashAccount>,
}

#[derive(Deserialize, Serialize)]
/// Used as an intermediate for serializing and deserializing account fields
/// into a human readable format.
struct SerdeAccount {
    pubkey: String,
    hash: String,
    owner: String,
    lamports: u64,
    rent_epoch: Epoch,
    executable: bool,
    data: String,
}

impl From<&PubkeyHashAccount> for SerdeAccount {
    fn from(pubkey_hash_account: &PubkeyHashAccount) -> Self {
        let PubkeyHashAccount {
            pubkey,
            hash,
            account,
        } = pubkey_hash_account;
        Self {
            pubkey: pubkey.to_string(),
            hash: hash.to_string(),
            owner: account.owner().to_string(),
            lamports: account.lamports(),
            rent_epoch: account.rent_epoch(),
            executable: account.executable(),
            data: BASE64_STANDARD.encode(account.data()),
        }
    }
}

impl TryFrom<SerdeAccount> for PubkeyHashAccount {
    type Error = String;

    fn try_from(temp_account: SerdeAccount) -> Result<Self, Self::Error> {
        let pubkey = Pubkey::from_str(&temp_account.pubkey).map_err(|err| err.to_string())?;
        let hash = Hash::from_str(&temp_account.hash).map_err(|err| err.to_string())?;

        let account = AccountSharedData::from(Account {
            lamports: temp_account.lamports,
            data: BASE64_STANDARD
                .decode(temp_account.data)
                .map_err(|err| err.to_string())?,
            owner: Pubkey::from_str(&temp_account.owner).map_err(|err| err.to_string())?,
            executable: temp_account.executable,
            rent_epoch: temp_account.rent_epoch,
        });

        Ok(Self {
            pubkey,
            hash,
            account,
        })
    }
}

impl Serialize for BankHashAccounts {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut seq = serializer.serialize_seq(Some(self.accounts.len()))?;
        for account in self.accounts.iter() {
            let temp_account = SerdeAccount::from(account);
            seq.serialize_element(&temp_account)?;
        }
        seq.end()
    }
}

impl<'de> Deserialize<'de> for BankHashAccounts {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let temp_accounts: Vec<SerdeAccount> = Deserialize::deserialize(deserializer)?;
        let pubkey_hash_accounts: Result<Vec<_>, _> = temp_accounts
            .into_iter()
            .map(PubkeyHashAccount::try_from)
            .collect();
        let pubkey_hash_accounts = pubkey_hash_accounts.map_err(de::Error::custom)?;
        Ok(BankHashAccounts {
            accounts: pubkey_hash_accounts,
        })
    }
}

/// Output the components that comprise bank hash
pub fn write_bank_hash_details_file(bank: &Bank) -> std::result::Result<(), String> {
    let details = BankHashDetails::try_from(bank)?;

    let slot = details.slot;
    let hash = &details.bank_hash;
    let file_name = format!("{slot}-{hash}.json");
    let parent_dir = bank
        .rc
        .accounts
        .accounts_db
        .get_base_working_path()
        .join("bank_hash_details");
    let path = parent_dir.join(file_name);
    // A file with the same name implies the same hash for this slot. Skip
    // rewriting a duplicate file in this scenario
    if !path.exists() {
        info!("writing details of bank {} to {}", slot, path.display());

        // std::fs::write may fail (depending on platform) if the full directory
        // path does not exist. So, call std::fs_create_dir_all first.
        // https://doc.rust-lang.org/std/fs/fn.write.html
        _ = std::fs::create_dir_all(parent_dir);
        let file = std::fs::File::create(&path)
            .map_err(|err| format!("Unable to create file at {}: {err}", path.display()))?;
        serde_json::to_writer_pretty(file, &details)
            .map_err(|err| format!("Unable to write file at {}: {err}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
pub mod tests {
    use super::*;

    #[test]
    fn test_serde_bank_hash_details() {
        use solana_sdk::hash::hash;

        let slot = 123_456_789;
        let signature_count = 314;

        let account = AccountSharedData::from(Account {
            lamports: 123_456_789,
            data: vec![0, 9, 1, 8, 2, 7, 3, 6, 4, 5],
            owner: Pubkey::new_unique(),
            executable: true,
            rent_epoch: 123,
        });
        let account_pubkey = Pubkey::new_unique();
        let account_hash = hash("account".as_bytes());
        let accounts = BankHashAccounts {
            accounts: vec![PubkeyHashAccount {
                pubkey: account_pubkey,
                hash: account_hash,
                account,
            }],
        };

        let bank_hash = hash("bank".as_bytes());
        let parent_bank_hash = hash("parent_bank".as_bytes());
        let accounts_delta_hash = hash("accounts_delta".as_bytes());
        let last_blockhash = hash("last_blockhash".as_bytes());

        let bank_hash_details = BankHashDetails::new(
            slot,
            bank_hash,
            parent_bank_hash,
            accounts_delta_hash,
            signature_count,
            last_blockhash,
            accounts,
        );

        let serialized_bytes = serde_json::to_vec(&bank_hash_details).unwrap();
        let deserialized_bank_hash_details: BankHashDetails =
            serde_json::from_slice(&serialized_bytes).unwrap();

        assert_eq!(bank_hash_details, deserialized_bank_hash_details);
    }
}
