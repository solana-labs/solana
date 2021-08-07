//! Cache of finalized address maps

use {
    dashmap::DashMap,
    solana_address_map_program::{AddressMap, DEACTIVATION_COOLDOWN},
    solana_sdk::{
        clock::Epoch,
        message::{v0, MappedAddresses},
        pubkey::Pubkey,
    },
};

#[derive(Debug)]
pub enum AddressMapChange {
    Activation(Pubkey),
    Deactivation(Pubkey),
}

// Address maps can be removed but they have to be deactivated. We need to store
// an activation and deactivation epoch.

/// Cached address map with an optional deactivation epoch.
#[derive(Debug)]
struct CachedAddressMap {
    entries: Vec<Pubkey>,
    deactivation_epoch: Option<Epoch>,
}

#[derive(Debug, Default)]
pub struct AddressMapCache {
    address_maps: DashMap<Pubkey, CachedAddressMap>,
}

impl AddressMapCache {
    pub fn map_message_addresses(&self, message: &v0::Message) -> Option<MappedAddresses> {
        let mut mapped_addresses = MappedAddresses {
            writable: Vec::with_capacity(message.num_writable_map_indexes()),
            readonly: Vec::with_capacity(message.num_readonly_map_indexes()),
        };

        for (key, indexes) in message.address_map_indexes_iter() {
            let address_map = self.address_maps.get(key)?;
            let lookup_address = |index: &u8| address_map.entries.get(*index as usize).cloned();

            mapped_addresses.writable.extend(
                indexes
                    .writable
                    .iter()
                    .map(lookup_address)
                    .collect::<Option<Vec<_>>>()?,
            );

            mapped_addresses.readonly.extend(
                indexes
                    .readonly
                    .iter()
                    .map(lookup_address)
                    .collect::<Option<Vec<_>>>()?,
            );
        }

        Some(mapped_addresses)
    }

    pub(crate) fn update_cache(&self, pubkey: &Pubkey, account_data: &[u8]) {
        if let Ok(address_map) = bincode::deserialize::<AddressMap>(account_data) {
            let deactivation_epoch = match address_map.deactivation_epoch {
                Epoch::MAX => None,
                epoch => Some(epoch),
            };

            // assumes that address maps cannot be recreated at the same account address
            if address_map.activated {
                self.address_maps
                    .entry(*pubkey)
                    .and_modify(|cached_address_map| {
                        // assumes that address map entries cannot be modified after activation
                        cached_address_map.deactivation_epoch = deactivation_epoch;
                    })
                    .or_insert(CachedAddressMap {
                        entries: address_map.entries,
                        deactivation_epoch,
                    });
            }
        } else {
            // remove closed address map
            self.address_maps.remove(pubkey);
        }
    }

    /// Purge all cached address maps that have been deactivated and passed the cooldown period
    pub fn purge_deactivated_maps(&self, current_epoch: Epoch) {
        self.address_maps.retain(|_address, address_map| {
            address_map
                .deactivation_epoch
                .map(|deactivation_epoch| {
                    deactivation_epoch + DEACTIVATION_COOLDOWN > current_epoch
                })
                .unwrap_or(true)
        });
    }
}
