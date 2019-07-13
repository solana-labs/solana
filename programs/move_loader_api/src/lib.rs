const MOVE_LOADER_PROGRAM_ID: [u8; 32] = [
    5, 91, 237, 31, 90, 253, 197, 145, 157, 236, 147, 43, 6, 5, 157, 238, 63, 151, 181, 165, 118,
    224, 198, 97, 103, 136, 113, 64, 0, 0, 0, 0,
];

solana_sdk::solana_name_id!(
    MOVE_LOADER_PROGRAM_ID,
    "MvLdr11111111111111111111111111111111111111"
);

use log::*;
use solana_sdk::account::KeyedAccount;
use solana_sdk::instruction::InstructionError;
use solana_sdk::loader_instruction::LoaderInstruction;
use solana_sdk::pubkey::Pubkey;
//use types::account_address::AccountAddress;
use types::transaction::TransactionArgument;
//use vm::file_format::CompiledModule;
use language_e2e_tests::compile_and_execute;
use serde::{Deserialize, Serialize};
use std::str;

#[derive(Serialize, Deserialize)]
enum MoveAccountData {
    Script { code: String },
    CompiledModule { bytecode: Vec<u8> },
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

                //// Executable accounts contain Move modules.
                //// The remaining accounts are used to pass data into the Move modules.

                //// TODO: Check that keyed_accounts[0].owner is the Move program id.

                //// TODO: Convert keyed_accounts[0].key (a Solana Pubkey) into a Libra AccountAddress?
                //let address = AccountAddress::default();

                // TODO: Return an error
                let args: Vec<TransactionArgument> = bincode::deserialize(&data).unwrap();

                let (programs, _params) = keyed_accounts.split_at_mut(1);

                //let compiled_module =
                //    CompiledModule::deserialize(&programs[0].account.data).unwrap();
                let account_data: MoveAccountData =
                    bincode::deserialize(&programs[0].account.data).unwrap();
                if let MoveAccountData::Script { code } = account_data {
                    // TODO: Handle there errors
                    compile_and_execute(&code, args).unwrap().unwrap();
                } else {
                    warn!("Unexpected Move account data");
                    return Err(InstructionError::GenericError);
                }

                //let (script, modules) =
                //    static_verify_program(&address, program.code(), compiled_program.modules())?;

                //// TODO: Provide a data view of the remaining non-executable accounts.
                //let mut data_view = FakeDataStore::default();
                //data_view.set(
                //    AccessPath::new(AccountAddress::random(), vec![]),
                //    vec![0, 0],
                //);
                //execute_function(script, modules, args, &data_view)?;

                info!("Call Move program");
            }
        }
    } else {
        warn!("Invalid program transaction: {:?}", ix_data);
        return Err(InstructionError::GenericError);
    }
    Ok(())
}
