use {super::*, std::collections::HashSet};

#[derive(Default, Clone, PartialEq, Debug, Deserialize, Serialize, AbiExample)]
pub(crate) struct UnusedAccounts {
    unused1: HashSet<Pubkey>,
    unused2: HashSet<Pubkey>,
    unused3: HashMap<Pubkey, u64>,
}
