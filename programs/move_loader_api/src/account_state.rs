use crate::data_store::DataStore;
use crate::error_mappers::*;
use bytecode_verifier::VerifiedModule;
use compiler::Compiler;
use serde_derive::{Deserialize, Serialize};
use solana_sdk::{instruction::InstructionError, pubkey::Pubkey};
use std::convert::TryInto;
use stdlib::stdlib_modules;
use types::{
    account_address::AccountAddress,
    account_config,
    identifier::Identifier,
    transaction::{Module, Script},
    write_set::{WriteOp, WriteSet},
};
use vm::{
    access::ModuleAccess, file_format::CompiledModule, transaction_metadata::TransactionMetadata,
};
use vm_cache_map::Arena;
use vm_runtime::{
    code_cache::{
        module_adapter::FakeFetcher,
        module_cache::{BlockModuleCache, VMModuleCache},
    },
    data_cache::BlockDataCache,
    txn_executor::{TransactionExecutor, ACCOUNT_MODULE, BLOCK_MODULE, COIN_MODULE},
};
use vm_runtime_types::value::Value;

// Helper function that converts a Solana Pubkey to a Libra AccountAddress (WIP)
pub fn pubkey_to_address(key: &Pubkey) -> AccountAddress {
    AccountAddress::new(*to_array_32(key.as_ref()))
}
fn to_array_32(array: &[u8]) -> &[u8; 32] {
    array.try_into().expect("slice with incorrect length")
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ModuleBytes {
    #[serde(with = "serde_bytes")]
    pub bytes: Vec<u8>,
}

/// Type of Libra account held by a Solana account
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum LibraAccountState {
    /// No data for this account yet
    Unallocated,
    /// Json string representation of types::transaction::Module
    CompiledScript(String),
    /// Json string representation of types::transaction::Script
    CompiledModule(String),
    /// Serialized verified script bytes
    VerifiedScript {
        #[serde(with = "serde_bytes")]
        script_bytes: Vec<u8>,
    },
    // Write set containing the published module
    PublishedModule(WriteSet),
    /// Associated genesis account and the write set containing the Libra account data
    User(Pubkey, WriteSet),
    /// Write sets containing the mint and stdlib modules
    Genesis(WriteSet),
}
impl LibraAccountState {
    pub fn create_unallocated() -> Self {
        Self::Unallocated
    }

    fn create_compiler(sender_address: &AccountAddress, deps: Vec<&Vec<u8>>) -> Compiler {
        // Compiler needs all the dependencies and the dependency module's account's
        // data into `VerifiedModules`
        let mut extra_deps: Vec<VerifiedModule> = vec![];
        for dep in deps {
            let state: Self = bincode::deserialize(&dep).unwrap();
            if let Self::PublishedModule(write_set) = state {
                for (_, write_op) in write_set.iter() {
                    if let WriteOp::Value(raw_bytes) = write_op {
                        let v =
                            VerifiedModule::new(CompiledModule::deserialize(&raw_bytes).unwrap())
                                .unwrap();
                        extra_deps.push(v);
                    }
                }
            }
        }

        Compiler {
            address: *sender_address,
            extra_deps,
            ..Compiler::default()
        }
    }

    pub fn create_script(sender_address: &AccountAddress, code: &str, deps: Vec<&Vec<u8>>) -> Self {
        let compiler = Self::create_compiler(sender_address, deps);
        let compiled_script = compiler.into_script(code).expect("Failed to compile");

        let mut script_bytes = vec![];
        compiled_script
            .serialize(&mut script_bytes)
            .expect("Unable to serialize script");

        // TODO args?
        Self::CompiledScript(serde_json::to_string(&Script::new(script_bytes, vec![])).unwrap())
    }

    pub fn create_module(sender_address: &AccountAddress, code: &str, deps: Vec<&Vec<u8>>) -> Self {
        let compiler = Self::create_compiler(sender_address, deps);
        let compiled_module = compiler
            .into_compiled_module(code)
            .expect("Failed to compile");

        let mut module_bytes = vec![];
        compiled_module
            .serialize(&mut module_bytes)
            .expect("Unable to serialize script");

        Self::CompiledModule(serde_json::to_string(&Module::new(module_bytes)).unwrap())
    }

    pub fn create_user(owner: &Pubkey, write_set: WriteSet) -> Self {
        Self::User(*owner, write_set)
    }

    pub fn create_genesis(mint_balance: u64) -> Result<(Self), InstructionError> {
        let modules = stdlib_modules();
        let arena = Arena::new();
        let state_view = DataStore::default();
        let vm_cache = VMModuleCache::new(&arena);
        let genesis_addr = account_config::association_address();

        let write_set = {
            let fake_fetcher =
                FakeFetcher::new(modules.iter().map(|m| m.as_inner().clone()).collect());
            let data_cache = BlockDataCache::new(&state_view);
            let block_cache = BlockModuleCache::new(&vm_cache, fake_fetcher);

            let mut txn_data = TransactionMetadata::default();
            txn_data.sender = genesis_addr;

            let mut txn_executor = TransactionExecutor::new(&block_cache, &data_cache, txn_data);
            txn_executor.create_account(genesis_addr).unwrap();
            txn_executor
                .create_account(account_config::core_code_address())
                .map_err(map_err_vm_status)?;
            txn_executor
                .execute_function(
                    &BLOCK_MODULE,
                    &Identifier::new("initialize").unwrap(),
                    vec![],
                )
                .map_err(map_err_vm_status)?;
            txn_executor
                .execute_function(
                    &COIN_MODULE,
                    &Identifier::new("initialize").unwrap(),
                    vec![],
                )
                .map_err(map_err_vm_status)?;

            txn_executor
                .execute_function(
                    &ACCOUNT_MODULE,
                    &Identifier::new("mint_to_address").unwrap(),
                    vec![Value::address(genesis_addr), Value::u64(mint_balance)],
                )
                .map_err(map_err_vm_status)?;

            // Bump the sequence number for the Association account. If we don't do this and a
            // subsequent transaction (e.g., minting) is sent from the Association account, a problem
            // arises: both the genesis transaction and the subsequent transaction have sequence
            // number 0
            txn_executor
                .execute_function(
                    &ACCOUNT_MODULE,
                    &Identifier::new("epilogue").unwrap(),
                    vec![],
                )
                .map_err(map_err_vm_status)?;

            let mut stdlib_modules = vec![];
            for module in modules.iter() {
                let mut buf = vec![];
                module.serialize(&mut buf).map_err(map_failure_error)?;
                stdlib_modules.push((module.self_id(), buf));
            }

            txn_executor
                .make_write_set(stdlib_modules, Ok(()))
                .map_err(map_err_vm_status)?
                .write_set()
                .clone()
                .into_mut()
        }
        .freeze()
        .map_err(map_failure_error)?;

        Ok(Self::Genesis(write_set))
    }
}
