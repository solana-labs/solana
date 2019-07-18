// TODO
#![allow(dead_code)]

use failure::prelude::*;
use state_view::StateView;
use std::collections::HashMap;
use types::{
    access_path::AccessPath,
    language_storage::ModuleId,
    write_set::{WriteOp, WriteSet},
};
use vm::{errors::*, CompiledModule};
use vm_runtime::data_cache::RemoteCache;

/// An in-memory implementation of [`StateView`] and [`RemoteCache`] for the VM.
#[derive(Debug, Default)]
pub struct DataStore {
    data: HashMap<AccessPath, Vec<u8>>,
}

impl DataStore {
    /// Creates a new `DataStore` with the provided initial data.
    pub fn new(data: HashMap<AccessPath, Vec<u8>>) -> Self {
        DataStore { data }
    }

    /// Applies a [`WriteSet`] to this data store.
    pub fn apply_write_set(&mut self, write_set: &WriteSet) {
        for (access_path, write_op) in write_set {
            match write_op {
                WriteOp::Value(blob) => {
                    self.set(access_path.clone(), blob.clone());
                }
                WriteOp::Deletion => {
                    self.remove(access_path);
                }
            }
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
        let mut blob = vec![];
        module
            .serialize(&mut blob)
            .expect("serializing this module should work");
        self.set(access_path, blob);
    }
}

// This is used by the `execute_block` API.
// TODO: only the "sync" get is implemented
impl StateView for DataStore {
    fn get(&self, access_path: &AccessPath) -> Result<Option<Vec<u8>>> {
        // Since the data is in-memory, it can't fail.
        match self.data.get(access_path) {
            None => Ok(None),
            Some(blob) => Ok(Some(blob.clone())),
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
    fn get(
        &self,
        access_path: &AccessPath,
    ) -> ::std::result::Result<Option<Vec<u8>>, VMInvariantViolation> {
        Ok(StateView::get(self, access_path).expect("it should not error"))
    }
}
