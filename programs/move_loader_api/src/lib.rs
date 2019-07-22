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

/// Type of exchange account, account's user data is populated with this enum
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum LibraAccountState {
    /// No data for this account yet
    Unallocated,
    /// Write set containing a Libra account's data
    WriteSet(WriteSet),
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

fn to_array_32(array: &[u8]) -> &[u8; 32] {
    array.try_into().expect("slice with incorrect length")
}

pub fn execute(
    script: VerifiedScript,
    args: Vec<TransactionArgument>,
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
    let txn_metadata = TransactionMetadata::default();

    let mut vm = TransactionExecutor::new(&module_cache, data_store, txn_metadata);
    let result = vm.execute_function(&module_id, &"main".to_string(), arguments_to_locals(args));
    vm.make_write_set(vec![], result).unwrap()
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
                const PROGRAM_INDEX: usize = 0;
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
                const PROGRAM_INDEX: usize = 0;
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
                const PROGRAM_INDEX: usize = 0;
                const GENESIS_INDEX: usize = 1;
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

                let args: Vec<TransactionArgument> = bincode::deserialize(&data).unwrap();

                // Dump Libra account data into data store
                let mut data_store = DataStore::default();
                for keyed_account in keyed_accounts[GENESIS_INDEX..].iter() {
                    if let LibraAccountState::WriteSet(write_set) =
                        bincode::deserialize(&keyed_account.account.data).unwrap()
                    {
                        data_store.apply_write_set(&write_set);
                    }
                }

                let program: Program =
                    serde_json::from_slice(&keyed_accounts[0].account.data).unwrap();
                let compiled_script = CompiledScript::deserialize(program.code()).unwrap();
                let modules = vec![];

                let sender_address = AccountAddress::default();
                // TODO: This function calls `.expect()` internally, need an error friendly version
                let (verified_script, modules) =
                    static_verify_program(&sender_address, compiled_script, modules)
                        .expect("verification failure");

                let output = execute(verified_script, args, modules, &data_store);
                for event in output.events() {
                    debug!("Event: {:?}", event);
                }
                if let TransactionStatus::Discard(status) = output.status() {
                    error!("Execution failed: {:?}", status);
                    return Err(InstructionError::GenericError);
                }
                data_store.apply_write_set(&output.write_set());

                // Dump Libra account data back into Solana account data
                let mut write_sets = data_store.into_write_sets();
                for (i, keyed_account) in keyed_accounts[GENESIS_INDEX..].iter_mut().enumerate() {
                    let address = if i == 0 {
                        // TODO: Remove this special case for genesis when genesis contains real pubkey
                        AccountAddress::default()
                    } else {
                        // let mut address = [0_u8; 32];
                        // address.copy_from_slice(keyed_account.unsigned_key().as_ref());
                        AccountAddress::new(*to_array_32(keyed_account.unsigned_key().as_ref()))
                    };
                    let write_set = write_sets.remove(&address).unwrap();
                    keyed_account.account.data.clear();
                    let writer = std::io::BufWriter::new(&mut keyed_account.account.data);
                    bincode::serialize_into(writer, &LibraAccountState::WriteSet(write_set))
                        .unwrap();
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
    use lazy_static::lazy_static;
    use proto_conv::FromProto;
    use protobuf::parse_from_bytes;
    use solana_sdk::account::Account;
    use std::{fs::File, io::prelude::*, path::PathBuf};
    use types::transaction::{SignedTransaction, TransactionPayload};

    // TODO: Cleanup copypasta.

    #[test]
    fn test_invoke_main() {
        solana_logger::setup();

        let program_id = Pubkey::new(&MOVE_LOADER_PROGRAM_ID);

        let code = "main() { return; }";

        let address = AccountAddress::default();
        let compiler = Compiler {
            code,
            address,
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

        let move_program_pubkey = Pubkey::new_rand();
        let program = Program::new(script, modules, vec![]);
        let program_bytes = serde_json::to_vec(&program).unwrap();
        let mut move_program_account = Account {
            lamports: 1,
            data: program_bytes,
            owner: program_id,
            executable: true,
        };

        let (genesis_pubkey, mut genesis_account) = get_genesis();

        let mut keyed_accounts = vec![
            KeyedAccount::new(&move_program_pubkey, false, &mut move_program_account),
            KeyedAccount::new(&genesis_pubkey, false, &mut genesis_account),
        ];

        let args: Vec<TransactionArgument> = vec![];
        let data = bincode::serialize(&args).unwrap();
        let ix = LoaderInstruction::InvokeMain { data };
        let ix_data = bincode::serialize(&ix).unwrap();

        // Ensure no panic
        process_instruction(&program_id, &mut keyed_accounts, &ix_data).unwrap();
    }

    #[test]
    #[ignore]
    fn test_mint() {
        solana_logger::setup();

        let program_id = Pubkey::new(&MOVE_LOADER_PROGRAM_ID);

        let code = "
            import 0x0.LibraAccount;
            import 0x0.LibraCoin;
            main(payee: address, amount: u64) {
                LibraAccount.mint_to_address(move(payee), move(amount));
                return;
            }
        ";

        let address = AccountAddress::default();
        let compiler = Compiler {
            code,
            address,
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

        let move_program_pubkey = Pubkey::new_rand();
        let program = Program::new(script, modules, vec![]);
        let program_bytes = serde_json::to_vec(&program).unwrap();
        let mut move_program_account = Account {
            lamports: 1,
            data: program_bytes,
            owner: program_id,
            executable: true,
        };

        let (genesis_pubkey, mut genesis_account) = get_genesis();

        let receiver_pubkey = Pubkey::new_rand();
        let receiver_bytes = bincode::serialize(&LibraAccountState::Unallocated).unwrap();
        let mut receiver_account = Account {
            lamports: 1,
            data: receiver_bytes,
            owner: program_id,
            executable: true,
        };

        let mut keyed_accounts = vec![
            KeyedAccount::new(&move_program_pubkey, false, &mut move_program_account),
            KeyedAccount::new(&genesis_pubkey, false, &mut genesis_account),
            KeyedAccount::new(&receiver_pubkey, false, &mut receiver_account),
        ];

        let receiver_addr = AccountAddress::new(to_array_32(receiver_pubkey.as_ref()).clone());
        let mint_amount = 42;
        let mut args: Vec<TransactionArgument> = Vec::new();
        args.push(TransactionArgument::Address(receiver_addr));
        args.push(TransactionArgument::U64(mint_amount));
        let data = bincode::serialize(&args).unwrap();

        let ix = LoaderInstruction::InvokeMain { data };
        let ix_data = bincode::serialize(&ix).unwrap();

        process_instruction(&program_id, &mut keyed_accounts, &ix_data).unwrap();

        let mut data_store = DataStore::default();
        match bincode::deserialize(&keyed_accounts[2].account.data).unwrap() {
            LibraAccountState::Unallocated => panic!("Invalid account state"),
            LibraAccountState::WriteSet(write_set) => data_store.apply_write_set(&write_set),
        }
        let updated_receiver = data_store.read_account_resource(&receiver_addr).unwrap();

        assert_eq!(42, AccountResource::read_balance(&updated_receiver));
        assert_eq!(0, AccountResource::read_sequence_number(&updated_receiver));
    }

    // TODO: Need a mechanism to initially encode the genesis and enforce it

    lazy_static! {
        /// The write set encoded in the genesis transaction.
    static ref GENESIS_WRITE_SET: WriteSet = {
            let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            path.pop();
            path.push("move_loader_api/src/genesis.blob");

            let mut f = File::open(&path).unwrap();
            let mut bytes = vec![];
            f.read_to_end(&mut bytes).unwrap();
            let txn = SignedTransaction::from_proto(parse_from_bytes(&bytes).unwrap()).unwrap();
            match txn.payload() {
                TransactionPayload::WriteSet(ws) => ws.clone(),
                _ => panic!("Expected writeset txn in genesis txn"),
            }
        };
    }

    fn get_genesis() -> (Pubkey, Account) {
        let genesis_pubkey = Pubkey::new_rand();
        let write_set = GENESIS_WRITE_SET.clone();
        let genesis_bytes = bincode::serialize(&LibraAccountState::WriteSet(write_set))
            .expect("Failed to serialize genesis WriteSet");
        let genesis_account = Account {
            lamports: 1,
            data: genesis_bytes,
            owner: Pubkey::new(&MOVE_LOADER_PROGRAM_ID),
            executable: false,
        };
        (genesis_pubkey, genesis_account)
    }
}
