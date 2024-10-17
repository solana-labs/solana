use {
    super::Bank,
    solana_sdk::{
        address_lookup_table::error::AddressLookupError,
        clock::Slot,
        message::{
            v0::{LoadedAddresses, MessageAddressTableLookup},
            AddressLoaderError,
        },
        transaction::AddressLoader,
    },
    solana_svm_transaction::message_address_table_lookup::SVMMessageAddressTableLookup,
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
        self.load_addresses_from_ref(
            address_table_lookups
                .iter()
                .map(SVMMessageAddressTableLookup::from),
        )
        .map(|(loaded_addresses, _deactivation_slot)| loaded_addresses)
    }
}

impl Bank {
    /// Load addresses from an iterator of `SVMMessageAddressTableLookup`,
    /// additionally returning the minimum deactivation slot across all referenced ALTs
    pub fn load_addresses_from_ref<'a>(
        &self,
        address_table_lookups: impl Iterator<Item = SVMMessageAddressTableLookup<'a>>,
    ) -> Result<(LoadedAddresses, Slot), AddressLoaderError> {
        let slot_hashes = self
            .transaction_processor
            .sysvar_cache()
            .get_slot_hashes()
            .map_err(|_| AddressLoaderError::SlotHashesSysvarNotFound)?;

        let mut deactivation_slot = u64::MAX;
        let mut loaded_addresses = LoadedAddresses::default();
        for address_table_lookup in address_table_lookups {
            deactivation_slot = deactivation_slot.min(
                self.rc
                    .accounts
                    .load_lookup_table_addresses_into(
                        &self.ancestors,
                        address_table_lookup,
                        &slot_hashes,
                        &mut loaded_addresses,
                    )
                    .map_err(into_address_loader_error)?,
            );
        }

        Ok((loaded_addresses, deactivation_slot))
    }
}
