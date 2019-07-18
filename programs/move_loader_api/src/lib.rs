mod data_store;

const MOVE_LOADER_PROGRAM_ID: [u8; 32] = [
    5, 91, 237, 31, 90, 253, 197, 145, 157, 236, 147, 43, 6, 5, 157, 238, 63, 151, 181, 165, 118,
    224, 198, 97, 103, 136, 113, 64, 0, 0, 0, 0,
];

solana_sdk::solana_name_id!(
    MOVE_LOADER_PROGRAM_ID,
    "MvLdr11111111111111111111111111111111111111"
);

use bytecode_verifier::{VerifiedModule, VerifiedScript};
use data_store::DataStore;
use log::*;
use solana_sdk::{
    account::KeyedAccount, instruction::InstructionError, loader_instruction::LoaderInstruction,
    pubkey::Pubkey,
};
use types::{
    account_address::AccountAddress,
    transaction::{Program, TransactionArgument, TransactionOutput},
};
use vm::{
    access::ModuleAccess, errors::*, file_format::CompiledScript,
    transaction_metadata::TransactionMetadata,
};
use vm_cache_map::Arena;
use vm_runtime::static_verify_program;
use vm_runtime::{
    code_cache::module_cache::{ModuleCache, VMModuleCache},
    data_cache::RemoteCache,
    txn_executor::TransactionExecutor,
};

pub fn execute(
    script: VerifiedScript,
    _args: Vec<TransactionArgument>,
    modules: Vec<VerifiedModule>,
    data_cache: &dyn RemoteCache,
) -> VMRuntimeResult<TransactionOutput> {
    let allocator = Arena::new();
    let module_cache = VMModuleCache::new(&allocator);
    for m in modules {
        module_cache.cache_module(m);
    }
    let main_module = script.into_module();
    let module_id = main_module.self_id();
    module_cache.cache_module(main_module);
    let txn_metadata = TransactionMetadata::default();

    let mut vm = TransactionExecutor::new(&module_cache, data_cache, txn_metadata);
    let result = vm.execute_function(&module_id, &"main".to_string(), [].to_vec());
    vm.make_write_set(vec![], result)
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
                if !keyed_accounts[0].account.executable {
                    warn!("Account not executable");
                    return Err(InstructionError::GenericError);
                }

                //// TODO: Check that keyed_accounts[0].owner is the Move program id.

                // TODO: Return an error
                let args: Vec<TransactionArgument> = bincode::deserialize(&data).unwrap();

                // TODO: Convert Accounts into data_cache.
                let mut data_cache = DataStore::default();

                let (programs, _params) = keyed_accounts.split_at_mut(1);
                let program: Program = serde_json::from_slice(&programs[0].account.data).unwrap();
                let compiled_script = CompiledScript::deserialize(program.code()).unwrap();
                let modules = vec![];

                let sender_address = AccountAddress::default();
                let (verified_script, modules) =
                    static_verify_program(&sender_address, compiled_script, modules)
                        .expect("verification failure");

                info!("Call Move program");
                let output = execute(verified_script, args, modules, &data_cache).unwrap();

                for event in output.events() {
                    error!("Event: {}", event);
                }

                data_cache.apply_write_set(&output.write_set());

                // TODO: convert data_cache back into accounts.
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
    use solana_sdk::account::Account;

    #[test]
    fn test_invoke_main() {
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

        let mut keyed_accounts = vec![KeyedAccount::new(
            &move_program_pubkey,
            false,
            &mut move_program_account,
        )];

        let args: Vec<TransactionArgument> = vec![];
        let data = bincode::serialize(&args).unwrap();
        let ix = LoaderInstruction::InvokeMain { data };
        let ix_data = bincode::serialize(&ix).unwrap();

        // Ensure no panic.
        process_instruction(&program_id, &mut keyed_accounts, &ix_data).unwrap();
    }
}
