use canonical_serialization::SimpleDeserializer;
use failure::prelude::*;
use indexmap::IndexMap;
use log::*;
use state_view::StateView;
use types::{
    access_path::AccessPath,
    account_address::AccountAddress,
    account_config::{self, AccountResource},
    language_storage::ModuleId,
    write_set::{WriteOp, WriteSet, WriteSetMut},
};
use vm::{errors::VMResult, CompiledModule};
use vm_runtime::{data_cache::RemoteCache, identifier::create_access_path};

/// An in-memory implementation of [`StateView`] and [`RemoteCache`] for the VM.
#[derive(Debug, Default)]
pub struct DataStore {
    data: IndexMap<AccessPath, Vec<u8>>,
}

impl DataStore {
    /// Creates a new `DataStore` with the provided initial data.
    pub fn new(data: IndexMap<AccessPath, Vec<u8>>) -> Self {
        Self { data }
    }

    /// Applies a [`WriteSet`] to this data store.
    pub fn apply_write_set(&mut self, write_set: &WriteSet) {
        for (access_path, write_op) in write_set {
            match write_op {
                WriteOp::Value(value) => {
                    self.set(access_path.clone(), value.clone());
                }
                WriteOp::Deletion => {
                    self.remove(access_path);
                }
            }
        }
    }

    /// Returns a `WriteSet` for each account in the `DataStore`
    pub fn into_write_sets(mut self) -> Result<IndexMap<AccountAddress, WriteSet>> {
        let mut write_set_muts: IndexMap<AccountAddress, WriteSetMut> = IndexMap::new();
        for (access_path, value) in self.data.drain(..) {
            match write_set_muts.get_mut(&access_path.address) {
                Some(write_set_mut) => write_set_mut.push((access_path, WriteOp::Value(value))),
                None => {
                    write_set_muts.insert(
                        access_path.address,
                        WriteSetMut::new(vec![(access_path, WriteOp::Value(value))]),
                    );
                }
            }
        }
        // Freeze each WriteSet
        let mut write_sets: IndexMap<AccountAddress, WriteSet> = IndexMap::new();
        for (address, write_set_mut) in write_set_muts.drain(..) {
            write_sets.insert(address, write_set_mut.freeze()?);
        }
        Ok(write_sets)
    }

    /// Read an account's resource
    pub fn read_account_resource(&self, addr: &AccountAddress) -> Option<AccountResource> {
        let access_path = create_access_path(&addr, account_config::account_struct_tag());
        match self.data.get(&access_path) {
            None => None,
            Some(blob) => SimpleDeserializer::deserialize(blob).ok(),
        }
    }

    /// Sets a (key, value) pair within this data store.
    ///
    /// Returns the previous data if the key was occupied.
    pub fn set(&mut self, access_path: AccessPath, data_blob: Vec<u8>) -> Option<Vec<u8>> {
        self.data.insert(access_path, data_blob)
    }

    /// Deletes a key from this data store.
    ///
    /// Returns the previous data if the key was occupied.
    pub fn remove(&mut self, access_path: &AccessPath) -> Option<Vec<u8>> {
        self.data.remove(access_path)
    }

    /// Adds a [`CompiledModule`] to this data store.
    ///
    /// Does not do any sort of verification on the module.
    pub fn add_module(&mut self, module_id: &ModuleId, module: &CompiledModule) {
        let access_path = AccessPath::from(module_id);
        let mut value = vec![];
        module
            .serialize(&mut value)
            .expect("serializing this module should work");
        self.set(access_path, value);
    }

    /// Dumps the data store to stdout
    pub fn dump(&self) {
        trace!("Data store:");
        for (access_path, value) in &self.data {
            trace!("{:?}: {:?}", access_path, value.len());
        }
    }
}

impl StateView for DataStore {
    fn get(&self, access_path: &AccessPath) -> Result<Option<Vec<u8>>> {
        // Since the data is in-memory, it can't fail.
        match self.data.get(access_path) {
            None => Ok(None),
            Some(value) => Ok(Some(value.clone())),
        }
    }

    fn multi_get(&self, _access_paths: &[AccessPath]) -> Result<Vec<Option<Vec<u8>>>> {
        unimplemented!();
    }

    fn is_genesis(&self) -> bool {
        false
    }
}

impl RemoteCache for DataStore {
    fn get(&self, access_path: &AccessPath) -> VMResult<Option<Vec<u8>>> {
        Ok(StateView::get(self, access_path).expect("it should not error"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use types::account_address::ADDRESS_LENGTH;

    #[test]
    fn test_write_set_order() {
        solana_logger::setup();

        let mut data_store = DataStore::default();
        let address1 = AccountAddress::new([0; ADDRESS_LENGTH]);
        let address2 = AccountAddress::new([1; ADDRESS_LENGTH]);
        let address3 = AccountAddress::new([2; ADDRESS_LENGTH]);

        let mut before1 = WriteSetMut::default();
        let mut before2 = WriteSetMut::default();
        let mut before3 = WriteSetMut::default();
        for i in 1..10 {
            before1.push((
                AccessPath::new(address1, AccountAddress::random().to_vec()),
                WriteOp::Value(vec![i]),
            ));
            before2.push((
                AccessPath::new(address2, AccountAddress::random().to_vec()),
                WriteOp::Value(vec![i]),
            ));
            before3.push((
                AccessPath::new(address3, AccountAddress::random().to_vec()),
                WriteOp::Value(vec![i]),
            ));
        }
        let before1 = before1.freeze().unwrap();
        let before2 = before2.freeze().unwrap();
        let before3 = before3.freeze().unwrap();
        data_store.apply_write_set(&before1);
        data_store.apply_write_set(&before2);
        data_store.apply_write_set(&before3);

        let write_sets = data_store.into_write_sets().unwrap();
        let after1 = write_sets.get(&address1).unwrap();
        let after2 = write_sets.get(&address2).unwrap();
        let after3 = write_sets.get(&address3).unwrap();

        assert_eq!(&before1, after1);
        assert_eq!(&before2, after2);
        assert_eq!(&before3, after3);
    }
}
