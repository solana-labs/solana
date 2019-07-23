const MOVE_LOADER_PROGRAM_ID: [u8; 32] = [
    5, 91, 237, 31, 90, 253, 197, 145, 157, 236, 147, 43, 6, 5, 157, 238, 63, 151, 181, 165, 118,
    224, 198, 97, 103, 136, 113, 64, 0, 0, 0, 0,
];

solana_sdk::solana_name_id!(
    MOVE_LOADER_PROGRAM_ID,
    "MvLdr11111111111111111111111111111111111111"
);

mod data_store;

use bytecode_verifier::{VerifiedModule, VerifiedScript};
use data_store::DataStore;
use log::*;
use serde_derive::{Deserialize, Serialize};
use solana_sdk::{
    account::KeyedAccount, instruction::InstructionError, loader_instruction::LoaderInstruction,
    pubkey::Pubkey,
};
use std::convert::TryInto;
use types::{
    account_address::AccountAddress,
    transaction::{Program, TransactionArgument, TransactionOutput, TransactionStatus},
    write_set::WriteSet,
};
use vm::{
    access::ModuleAccess, file_format::CompiledScript, transaction_metadata::TransactionMetadata,
};
use vm_cache_map::Arena;
use vm_runtime::{
    code_cache::{
        module_adapter::ModuleFetcherImpl,
        module_cache::{BlockModuleCache, ModuleCache, VMModuleCache},
    },
    static_verify_program,
    txn_executor::TransactionExecutor,
    value::Local,
};

const PROGRAM_INDEX: usize = 0;
const GENESIS_INDEX: usize = 1;

/// Type of Libra account held by a Solana account
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum LibraAccountState {
    /// No data for this account yet
    Unallocated,
    /// Program bits
    Program(Program),
    /// Write set containing a Libra account's data
    User(WriteSet),
    /// Write sets containing the mint and stdlib modules
    Genesis(WriteSet),
}

// TODO: Not quite right yet
/// Invoke information passed via the Invoke Instruction
#[derive(Default, Debug, Serialize, Deserialize)]
pub struct InvokeInfo {
    /// Sender of the 'transaction", the "sender" who is calling this program
    sender_address: AccountAddress,
    /// Arguments to pass to the program being invoked
    args: Vec<TransactionArgument>,
}

fn arguments_to_locals(args: Vec<TransactionArgument>) -> Vec<Local> {
    let mut locals = vec![];
    for arg in args.into_iter() {
        locals.push(match arg {
            TransactionArgument::U64(i) => Local::u64(i),
            TransactionArgument::Address(a) => Local::address(a),
            TransactionArgument::ByteArray(b) => Local::bytearray(b),
            TransactionArgument::String(s) => Local::string(s),
        });
    }
    locals
}

fn pubkey_to_address(key: &Pubkey) -> AccountAddress {
    AccountAddress::new(*to_array_32(key.as_ref()))
}
fn to_array_32(array: &[u8]) -> &[u8; 32] {
    array.try_into().expect("slice with incorrect length")
}

pub fn execute(
    invoke_info: InvokeInfo,
    script: VerifiedScript,
    modules: Vec<VerifiedModule>,
    data_store: &DataStore,
) -> TransactionOutput {
    let allocator = Arena::new();
    let code_cache = VMModuleCache::new(&allocator);
    let module_cache = BlockModuleCache::new(&code_cache, ModuleFetcherImpl::new(data_store));
    for m in modules {
        module_cache.cache_module(m);
    }
    let main_module = script.into_module();
    let module_id = main_module.self_id();
    module_cache.cache_module(main_module);
    let mut txn_metadata = TransactionMetadata::default();
    txn_metadata.sender = invoke_info.sender_address;

    let mut vm = TransactionExecutor::new(&module_cache, data_store, txn_metadata);
    let result = vm.execute_function(
        &module_id,
        &"main".to_string(),
        arguments_to_locals(invoke_info.args),
    );
    vm.make_write_set(vec![], result).unwrap()
}

fn keyed_accounts_to_data_store(keyed_accounts: &[KeyedAccount]) -> DataStore {
    let mut data_store = DataStore::default();
    for keyed_account in keyed_accounts {
        match bincode::deserialize(&keyed_account.account.data).unwrap() {
            LibraAccountState::Genesis(write_set) | LibraAccountState::User(write_set) => {
                data_store.apply_write_set(&write_set)
            }
            _ => (), // ignore unallocated accounts
        }
    }
    data_store
}

pub fn process_instruction(
    _program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    ix_data: &[u8],
) -> Result<(), InstructionError> {
    solana_logger::setup();

    if let Ok(instruction) = bincode::deserialize(ix_data) {
        match instruction {
            LoaderInstruction::Write { offset, bytes } => {
                if keyed_accounts[PROGRAM_INDEX].signer_key().is_none() {
                    warn!("key[0] did not sign the transaction");
                    return Err(InstructionError::GenericError);
                }
                let offset = offset as usize;
                let len = bytes.len();
                debug!("Write: offset={} length={}", offset, len);
                if keyed_accounts[PROGRAM_INDEX].account.data.len() < offset + len {
                    warn!(
                        "Write overflow: {} < {}",
                        keyed_accounts[PROGRAM_INDEX].account.data.len(),
                        offset + len
                    );
                    return Err(InstructionError::GenericError);
                }
                keyed_accounts[PROGRAM_INDEX].account.data[offset..offset + len]
                    .copy_from_slice(&bytes);
            }
            LoaderInstruction::Finalize => {
                if keyed_accounts[PROGRAM_INDEX].signer_key().is_none() {
                    warn!("key[0] did not sign the transaction");
                    return Err(InstructionError::GenericError);
                }
                keyed_accounts[PROGRAM_INDEX].account.executable = true;
                info!(
                    "Finalize: account {:?}",
                    keyed_accounts[PROGRAM_INDEX].signer_key().unwrap()
                );
            }
            LoaderInstruction::InvokeMain { data } => {
                if keyed_accounts.len() < 2 {
                    error!("Need at least program and genesis accounts");
                    return Err(InstructionError::InvalidArgument);
                }
                if keyed_accounts[PROGRAM_INDEX].account.owner
                    != Pubkey::new(&MOVE_LOADER_PROGRAM_ID)
                {
                    error!("Move program account not owned by Move loader");
                    return Err(InstructionError::InvalidArgument);
                }
                if !keyed_accounts[PROGRAM_INDEX].account.executable {
                    error!("Move program account not executable");
                    return Err(InstructionError::InvalidArgument);
                }

                // TODO: Return errors instead of panicking

                let invoke_info: InvokeInfo = bincode::deserialize(&data).unwrap();

                let program = match bincode::deserialize(&keyed_accounts[0].account.data).unwrap() {
                    LibraAccountState::Program(program) => program,
                    _ => {
                        error!("First account must contain the program bits");
                        return Err(InstructionError::InvalidArgument);
                    }
                };
                let compiled_script = CompiledScript::deserialize(program.code()).unwrap();
                // TODO: Add support for modules
                let modules = vec![];

                let mut data_store = keyed_accounts_to_data_store(&keyed_accounts[GENESIS_INDEX..]);

                let (verified_script, modules) =
                    // TODO: This function calls `.expect()` internally, need an error friendly version
                    static_verify_program(&invoke_info.sender_address, compiled_script, modules)
                        .expect("verification failure");

                let output = execute(invoke_info, verified_script, modules, &data_store);
                for event in output.events() {
                    debug!("Event: {:?}", event);
                }
                if let TransactionStatus::Discard(status) = output.status() {
                    error!("Execution failed: {:?}", status);
                    return Err(InstructionError::GenericError);
                }
                data_store.apply_write_set(&output.write_set());

                // Break data store into a list of address keyed WriteSets
                let mut write_sets = data_store.into_write_sets();

                // Genesis account holds both mint and stdliib under address 0x0
                let write_set = write_sets.remove(&AccountAddress::default()).unwrap();
                keyed_accounts[GENESIS_INDEX].account.data.clear();
                let writer =
                    std::io::BufWriter::new(&mut keyed_accounts[GENESIS_INDEX].account.data);
                bincode::serialize_into(writer, &LibraAccountState::Genesis(write_set)).unwrap();

                // Now do the rest of the accounts
                for keyed_account in keyed_accounts[GENESIS_INDEX + 1..].iter_mut() {
                    let write_set = write_sets
                        .remove(&pubkey_to_address(keyed_account.unsigned_key()))
                        .unwrap();
                    keyed_account.account.data.clear();
                    let writer = std::io::BufWriter::new(&mut keyed_account.account.data);
                    bincode::serialize_into(writer, &LibraAccountState::User(write_set)).unwrap();
                }
                if !write_sets.is_empty() {
                    error!("Missing keyed accounts");
                    return Err(InstructionError::GenericError);
                }
            }
        }
    } else {
        warn!("Invalid program transaction: {:?}", ix_data);
        return Err(InstructionError::GenericError);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use compiler::Compiler;
    use language_e2e_tests::account::AccountResource;
    use solana_sdk::account::Account;
    use stdlib::stdlib_modules;
    use types::byte_array::ByteArray;
    use vm_runtime::{
        code_cache::{
            module_adapter::FakeFetcher,
            module_cache::{BlockModuleCache, VMModuleCache},
        },
        data_cache::BlockDataCache,
        txn_executor::{TransactionExecutor, ACCOUNT_MODULE, COIN_MODULE},
    };

    #[test]
    fn test_invoke_main() {
        solana_logger::setup();

        let code = "main() { return; }";
        let mut program = LibraAccount::create_program(&AccountAddress::default(), code);
        let mut genesis = LibraAccount::create_genesis();

        let mut keyed_accounts = vec![
            KeyedAccount::new(&program.key, false, &mut program.account),
            KeyedAccount::new(&genesis.key, false, &mut genesis.account),
        ];
        call_process_instruction(&mut keyed_accounts, InvokeInfo::default());
    }

    #[test]
    fn test_invoke_mint_to_address() {
        solana_logger::setup();

        let amount = 42;
        let accounts = mint_coins(amount).unwrap();

        let mut data_store = DataStore::default();
        match bincode::deserialize(&accounts[GENESIS_INDEX + 1].account.data).unwrap() {
            LibraAccountState::User(write_set) => data_store.apply_write_set(&write_set),
            _ => panic!("Invalid account state"),
        }
        let payee_resource = data_store
            .read_account_resource(&accounts[GENESIS_INDEX + 1].address)
            .unwrap();

        assert_eq!(amount, AccountResource::read_balance(&payee_resource));
        assert_eq!(0, AccountResource::read_sequence_number(&payee_resource));
    }

    #[test]
    fn test_invoke_pay_from_sender() {
        let amount_to_mint = 42;
        let mut accounts = mint_coins(amount_to_mint).unwrap();

        let code = "
            import 0x0.LibraAccount;
            import 0x0.LibraCoin;
            main(payee: address, amount: u64) {
                LibraAccount.pay_from_sender(move(payee), move(amount));
                return;
            }
        ";
        let mut program = LibraAccount::create_program(&accounts[GENESIS_INDEX + 1].address, code);
        let mut payee = LibraAccount::create_unallocated();

        let (genesis, sender) = accounts.split_at_mut(GENESIS_INDEX + 1);
        let genesis = &mut genesis[1];
        let sender = &mut sender[0];
        let mut keyed_accounts = vec![
            KeyedAccount::new(&program.key, false, &mut program.account),
            KeyedAccount::new(&genesis.key, false, &mut genesis.account),
            KeyedAccount::new(&sender.key, false, &mut sender.account),
            KeyedAccount::new(&payee.key, false, &mut payee.account),
        ];

        let amount = 2;
        let invoke_info = InvokeInfo {
            sender_address: sender.address.clone(),
            args: vec![
                TransactionArgument::Address(payee.address.clone()),
                TransactionArgument::U64(amount),
            ],
        };

        call_process_instruction(&mut keyed_accounts, invoke_info);

        let data_store = keyed_accounts_to_data_store(&keyed_accounts[1..]);
        let sender_resource = data_store.read_account_resource(&sender.address).unwrap();
        let payee_resource = data_store.read_account_resource(&payee.address).unwrap();

        assert_eq!(
            amount_to_mint - amount,
            AccountResource::read_balance(&sender_resource)
        );
        assert_eq!(0, AccountResource::read_sequence_number(&sender_resource));
        assert_eq!(amount, AccountResource::read_balance(&payee_resource));
        assert_eq!(0, AccountResource::read_sequence_number(&payee_resource));
    }

    // Helpers

    fn mint_coins(amount: u64) -> Result<Vec<LibraAccount>, InstructionError> {
        let code = "
            import 0x0.LibraAccount;
            import 0x0.LibraCoin;
            main(payee: address, amount: u64) {
                LibraAccount.mint_to_address(move(payee), move(amount));
                return;
            }
        ";
        let mut genesis = LibraAccount::create_genesis();
        let mut program = LibraAccount::create_program(&genesis.address, code);
        let mut payee = LibraAccount::create_unallocated();

        let mut keyed_accounts = vec![
            KeyedAccount::new(&program.key, false, &mut program.account),
            KeyedAccount::new(&genesis.key, false, &mut genesis.account),
            KeyedAccount::new(&payee.key, false, &mut payee.account),
        ];
        let invoke_info = InvokeInfo {
            sender_address: genesis.address.clone(),
            args: vec![
                TransactionArgument::Address(pubkey_to_address(&payee.key)),
                TransactionArgument::U64(amount),
            ],
        };

        call_process_instruction(&mut keyed_accounts, invoke_info);

        Ok(vec![
            LibraAccount::new(program.key, program.account),
            LibraAccount::new(genesis.key, genesis.account),
            LibraAccount::new(payee.key, payee.account),
        ])
    }

    fn call_process_instruction(keyed_accounts: &mut [KeyedAccount], invoke_info: InvokeInfo) {
        let program_id = Pubkey::new(&MOVE_LOADER_PROGRAM_ID);

        let data = bincode::serialize(&invoke_info).unwrap();
        let ix = LoaderInstruction::InvokeMain { data };
        let ix_data = bincode::serialize(&ix).unwrap();

        process_instruction(&program_id, keyed_accounts, &ix_data).unwrap();
    }

    struct LibraAccount {
        pub key: Pubkey,
        pub address: AccountAddress,
        pub account: Account,
    }
    impl LibraAccount {
        pub fn new(key: Pubkey, account: Account) -> Self {
            let address = pubkey_to_address(&key);
            Self {
                key,
                address,
                account,
            }
        }

        pub fn create_unallocated() -> Self {
            let key = Pubkey::new_rand();
            let account = Account {
                lamports: 1,
                data: bincode::serialize(&LibraAccountState::Unallocated).unwrap(),
                owner: Pubkey::new(&MOVE_LOADER_PROGRAM_ID),
                executable: false,
            };
            Self::new(key, account)
        }

        pub fn create_genesis() -> Self {
            let account = Account {
                lamports: 1,
                data: vec![],
                owner: Pubkey::new(&MOVE_LOADER_PROGRAM_ID),
                executable: false,
            };
            let mut genesis = Self::new(Pubkey::default(), account);

            const INIT_BALANCE: u64 = 1_000_000_000;

            let modules = stdlib_modules();
            let arena = Arena::new();
            let state_view = DataStore::default();
            let vm_cache = VMModuleCache::new(&arena);
            let genesis_addr = genesis.address;
            let genesis_auth_key = ByteArray::new(genesis.address.clone().to_vec());

            let write_set = {
                let fake_fetcher =
                    FakeFetcher::new(modules.iter().map(|m| m.as_inner().clone()).collect());
                let data_cache = BlockDataCache::new(&state_view);
                let block_cache = BlockModuleCache::new(&vm_cache, fake_fetcher);
                {
                    let mut txn_data = TransactionMetadata::default();
                    txn_data.sender = genesis_addr;

                    let mut txn_executor =
                        TransactionExecutor::new(&block_cache, &data_cache, txn_data);
                    txn_executor.create_account(genesis_addr).unwrap().unwrap();
                    txn_executor
                        .execute_function(&COIN_MODULE, "initialize", vec![])
                        .unwrap()
                        .unwrap();

                    txn_executor
                        .execute_function(
                            &ACCOUNT_MODULE,
                            "mint_to_address",
                            vec![Local::address(genesis_addr), Local::u64(INIT_BALANCE)],
                        )
                        .unwrap()
                        .unwrap();

                    txn_executor
                        .execute_function(
                            &ACCOUNT_MODULE,
                            "rotate_authentication_key",
                            vec![Local::bytearray(genesis_auth_key)],
                        )
                        .unwrap()
                        .unwrap();

                    let stdlib_modules = modules
                        .iter()
                        .map(|m| {
                            let mut module_vec = vec![];
                            m.serialize(&mut module_vec).unwrap();
                            (m.self_id(), module_vec)
                        })
                        .collect();

                    txn_executor
                        .make_write_set(stdlib_modules, Ok(Ok(())))
                        .unwrap()
                        .write_set()
                        .clone()
                        .into_mut()
                }
            }
            .freeze()
            .unwrap();

            genesis.account.data = bincode::serialize(&LibraAccountState::Genesis(write_set))
                .expect("Failed to serialize genesis WriteSet");
            genesis
        }

        pub fn create_program(sender_address: &AccountAddress, code: &str) -> Self {
            let compiler = Compiler {
                address: sender_address.clone(),
                code,
                ..Compiler::default()
            };
            let compiled_program = compiler.into_compiled_program().expect("Failed to compile");

            let mut script = vec![];
            compiled_program
                .script
                .serialize(&mut script)
                .expect("Unable to serialize script");
            let mut modules = vec![];
            for m in compiled_program.modules.iter() {
                let mut buf = vec![];
                m.serialize(&mut buf).expect("Unable to serialize module");
                modules.push(buf);
            }
            let data = Program::new(script, modules, vec![]);

            let mut program = Self::create_unallocated();
            program.account.data = bincode::serialize(&LibraAccountState::Program(data)).unwrap();
            program.account.executable = true;
            program
        }
    }
}
