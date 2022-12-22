use {
    serde::{
        de::{self, Visitor},
        ser::SerializeMap,
        Deserialize, Serialize,
    },
    solana_sdk::{
        account::{Account, AccountSharedData},
        pubkey::{ParsePubkeyError, Pubkey},
        stake_history::Epoch,
        sysvar,
    },
    std::collections::HashMap,
};

/// Encapsulates overridden accounts, typically used for transaction simulations
#[derive(Default, PartialEq, Eq, Debug, Clone)]
pub struct AccountOverrides {
    accounts: HashMap<Pubkey, AccountSharedData>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccountDef {
    lamports: u64,
    data: Vec<u8>,
    owner: String,
    executable: bool,
    rent_epoch: Epoch,
}

impl From<AccountSharedData> for AccountDef {
    fn from(account: AccountSharedData) -> Self {
        let Account {
            lamports,
            data,
            owner,
            executable,
            rent_epoch,
        } = account.into();

        AccountDef {
            lamports,
            data,
            owner: format!("{}", owner),
            executable,
            rent_epoch,
        }
    }
}

impl TryFrom<AccountDef> for AccountSharedData {
    type Error = ParsePubkeyError;

    fn try_from(def: AccountDef) -> Result<Self, Self::Error> {
        Ok(Account {
            lamports: def.lamports,
            data: def.data,
            owner: def.owner.parse()?,
            executable: def.executable,
            rent_epoch: def.rent_epoch,
        }
        .into())
    }
}

impl Serialize for AccountOverrides {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut map = serializer.serialize_map(Some(self.accounts.len()))?;
        for (pubkey, account) in self.accounts.iter() {
            map.serialize_entry(&format!("{}", pubkey), &AccountDef::from(account.clone()))?;
        }
        map.end()
    }
}

struct AccountOverridesVisitor;

impl<'de> Visitor<'de> for AccountOverridesVisitor {
    type Value = AccountOverrides;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("A map of Pubkey to Account")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::MapAccess<'de>,
    {
        let mut accounts = HashMap::with_capacity(map.size_hint().unwrap_or(0));

        while let Some((pubkey, account)) = map.next_entry::<String, AccountDef>()? {
            accounts.insert(
                pubkey
                    .parse()
                    .map_err(|e| de::Error::custom(format!("{e}")))?,
                account
                    .try_into()
                    .map_err(|e| de::Error::custom(format!("{e}")))?,
            );
        }

        Ok(AccountOverrides { accounts })
    }
}

impl<'de> Deserialize<'de> for AccountOverrides {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_map(AccountOverridesVisitor)
    }
}

impl AccountOverrides {
    pub fn set_account(&mut self, pubkey: &Pubkey, account: Option<AccountSharedData>) {
        match account {
            Some(account) => self.accounts.insert(*pubkey, account),
            None => self.accounts.remove(pubkey),
        };
    }

    /// Sets in the slot history
    ///
    /// Note: no checks are performed on the correctness of the contained data
    pub fn set_slot_history(&mut self, slot_history: Option<AccountSharedData>) {
        self.set_account(&sysvar::slot_history::id(), slot_history);
    }

    /// Gets the account if it's found in the list of overrides
    pub fn get(&self, pubkey: &Pubkey) -> Option<&AccountSharedData> {
        self.accounts.get(pubkey)
    }
}
