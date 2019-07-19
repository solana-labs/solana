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
use solana_sdk::{
    account::KeyedAccount, instruction::InstructionError, loader_instruction::LoaderInstruction,
    pubkey::Pubkey,
};
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
                if keyed_accounts[0].signer_key().is_none() {
                    warn!("key[0] did not sign the transaction");
                    return Err(InstructionError::GenericError);
                }
                let offset = offset as usize;
                let len = bytes.len();
                debug!("Write: offset={} length={}", offset, len);
                if keyed_accounts[0].account.data.len() < offset + len {
                    warn!(
                        "Write overflow: {} < {}",
                        keyed_accounts[0].account.data.len(),
                        offset + len
                    );
                    return Err(InstructionError::GenericError);
                }
                keyed_accounts[0].account.data[offset..offset + len].copy_from_slice(&bytes);
            }
            LoaderInstruction::Finalize => {
                if keyed_accounts[0].signer_key().is_none() {
                    warn!("key[0] did not sign the transaction");
                    return Err(InstructionError::GenericError);
                }
                keyed_accounts[0].account.executable = true;
                info!(
                    "Finalize: account {:?}",
                    keyed_accounts[0].signer_key().unwrap()
                );
            }
            LoaderInstruction::InvokeMain { data } => {
                if keyed_accounts.len() < 2 {
                    error!("Need at least program and genesis accounts");
                    return Err(InstructionError::InvalidArgument);
                }
                if !keyed_accounts[0].account.executable {
                    error!("Account not executable");
                    return Err(InstructionError::InvalidArgument);
                }
                // TODO: Check that keyed_accounts[0].owner is the Move program id.

                // TODO: Return errors instead of panicking

                let args: Vec<TransactionArgument> = bincode::deserialize(&data).unwrap();

                let mut data_store = DataStore::default();
                let genesis_write_set: WriteSet =
                    bincode::deserialize(&keyed_accounts[1].account.data).unwrap();
                data_store.apply_write_set(&genesis_write_set);

                // TODO: Load Account data into the DataStore.

                let (programs, _params) = keyed_accounts.split_at_mut(1);
                let program: Program = serde_json::from_slice(&programs[0].account.data).unwrap();
                let compiled_script = CompiledScript::deserialize(program.code()).unwrap();
                let modules = vec![];

                let sender_address = AccountAddress::default();
                // TODO: Is `static_verify_program()` what we want?
                let (verified_script, modules) =
                    static_verify_program(&sender_address, compiled_script, modules)
                        .expect("verification failure");

                let output = execute(verified_script, args, modules, &data_store);
                for event in output.events() {
                    debug!("Event: {}", event);
                }
                if let TransactionStatus::Discard(status) = output.status() {
                    error!("Execution failed: {:?}", status);
                    return Err(InstructionError::GenericError);
                }
                data_store.apply_write_set(&output.write_set());

                // TODO: Dump DataStore back into Account data
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

        let program = Program::new(script, modules, vec![]);
        let program_bytes = serde_json::to_vec(&program).unwrap();

        let move_program_pubkey = Pubkey::new_rand();
        let mut move_program_account = Account {
            lamports: 1,
            data: program_bytes,
            owner: program_id,
            executable: true,
        };

        let (genesis_pubkey, mut genesis_account) = get_genesis(&program_id);

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

        let program = Program::new(script, modules, vec![]);
        let program_bytes = serde_json::to_vec(&program).unwrap();

        let move_program_pubkey = Pubkey::new_rand();
        let mut move_program_account = Account {
            lamports: 1,
            data: program_bytes,
            owner: program_id,
            executable: true,
        };

        let (genesis_pubkey, mut genesis_account) = get_genesis(&program_id);

        let mut keyed_accounts = vec![
            KeyedAccount::new(&move_program_pubkey, false, &mut move_program_account),
            KeyedAccount::new(&genesis_pubkey, false, &mut genesis_account),
        ];

        // Receiver's address
        let receiver_addr = AccountAddress::random();
        let mint_amount = 42;

        let mut args: Vec<TransactionArgument> = Vec::new();
        args.push(TransactionArgument::Address(receiver_addr));
        args.push(TransactionArgument::U64(mint_amount));
        let data = bincode::serialize(&args).unwrap();

        let ix = LoaderInstruction::InvokeMain { data };
        let ix_data = bincode::serialize(&ix).unwrap();

        process_instruction(&program_id, &mut keyed_accounts, &ix_data).unwrap();

        // TODO Check account results to ensure the mint program did the work expected
    }

    // TODO: Need a mechanism to initially encode the genesis and enforce it

    lazy_static! {
        /// The write set encoded in the genesis transaction.
        pub static ref GENESIS_WRITE_SET: WriteSet = {
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

    pub fn get_genesis(program_id: &Pubkey) -> (Pubkey, Account) {
        let genesis_pubkey = Pubkey::new_rand();
        let write_set: &WriteSet = &GENESIS_WRITE_SET;
        let genesis_bytes =
            bincode::serialize(write_set).expect("Failed to serialize genesis WriteSet");
        let genesis_account = Account {
            lamports: 1,
            data: genesis_bytes,
            owner: *program_id,
            executable: false,
        };
        (genesis_pubkey, genesis_account)
    }
}
