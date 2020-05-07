use solana_sdk::pubkey::Pubkey;

#[derive(Default, Debug, Clone, PartialEq)]
pub struct Supply {
    pub circulating: u64,
    pub non_circulating: u64,
    pub non_circulating_accounts: Vec<Pubkey>,
}
