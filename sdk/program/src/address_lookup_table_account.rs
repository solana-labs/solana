use solana_program::pubkey::Pubkey;

#[derive(Debug, PartialEq, Clone)]
pub struct AddressLookupTableAccount {
    pub key: Pubkey,
    pub addresses: Vec<Pubkey>,
}
