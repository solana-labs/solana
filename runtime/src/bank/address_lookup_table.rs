use {
    super::Bank,
    solana_sdk::{
        address_lookup_table::error::AddressLookupError,
        message::{
            v0::{LoadedAddresses, MessageAddressTableLookup},
            AddressLoaderError,
        },
        transaction::AddressLoader,
    },
};

fn into_address_loader_error(err: AddressLookupError) -> AddressLoaderError {
    match err {
        AddressLookupError::LookupTableAccountNotFound => {
            AddressLoaderError::LookupTableAccountNotFound
        }
        AddressLookupError::InvalidAccountOwner => AddressLoaderError::InvalidAccountOwner,
        AddressLookupError::InvalidAccountData => AddressLoaderError::InvalidAccountData,
        AddressLookupError::InvalidLookupIndex => AddressLoaderError::InvalidLookupIndex,
    }
}

impl AddressLoader for &Bank {
    fn load_addresses(
        self,
        address_table_lookups: &[MessageAddressTableLookup],
    ) -> Result<LoadedAddresses, AddressLoaderError> {
        let slot_hashes = self
            .transaction_processor
            .sysvar_cache
            .read()
            .unwrap()
            .get_slot_hashes()
            .map_err(|_| AddressLoaderError::SlotHashesSysvarNotFound)?;

        address_table_lookups
            .iter()
            .map(|address_table_lookup| {
                self.rc
                    .accounts
                    .load_lookup_table_addresses(
                        &self.ancestors,
                        address_table_lookup,
                        &slot_hashes,
                    )
                    .map_err(into_address_loader_error)
            })
            .collect::<Result<_, _>>()
    }
}
