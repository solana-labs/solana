use {
    super::Bank,
    crate::accounts_db::LoadZeroLamports,
    solana_address_lookup_table_program::error::AddressLookupError,
    solana_sdk::{
        message::{
            v0::{LoadedAddresses, MessageAddressTableLookup},
            AddressLoaderError,
        },
        transaction::AddressLoader,
    },
};

impl AddressLoader for &Bank {
    fn load_addresses(
        self,
        address_table_lookups: &[MessageAddressTableLookup],
    ) -> Result<LoadedAddresses, AddressLoaderError> {
        if !self.versioned_tx_message_enabled() {
            return Err(AddressLoaderError::Disabled);
        }

        let load_zero_lamports = LoadZeroLamports::None;

        let slot_hashes = self
            .sysvar_cache
            .read()
            .unwrap()
            .get_slot_hashes()
            .map_err(|_| AddressLoaderError::SlotHashesSysvarNotFound)?;

        Ok(address_table_lookups
            .iter()
            .map(|address_table_lookup| {
                self.rc.accounts.load_lookup_table_addresses(
                    &self.ancestors,
                    address_table_lookup,
                    &slot_hashes,
                    load_zero_lamports,
                )
            })
            .collect::<Result<_, AddressLookupError>>()?)
    }
}
