use {solana_accounts_db::accounts_hash::AccountsLtHash, solana_lattice_hash::lt_hash::LtHash};

/// Snapshot serde-safe AccountsLtHash
#[cfg_attr(feature = "frozen-abi", derive(AbiExample))]
#[serde_with::serde_as]
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct SerdeAccountsLtHash(
    // serde only has array support up to 32 elements; anything larger needs to be handled manually
    // see https://github.com/serde-rs/serde/issues/1937 for more information
    #[serde_as(as = "[_; LtHash::NUM_ELEMENTS]")] pub [u16; LtHash::NUM_ELEMENTS],
);

impl From<SerdeAccountsLtHash> for AccountsLtHash {
    fn from(accounts_lt_hash: SerdeAccountsLtHash) -> Self {
        Self(LtHash(accounts_lt_hash.0))
    }
}
impl From<AccountsLtHash> for SerdeAccountsLtHash {
    fn from(accounts_lt_hash: AccountsLtHash) -> Self {
        Self(accounts_lt_hash.0 .0)
    }
}
