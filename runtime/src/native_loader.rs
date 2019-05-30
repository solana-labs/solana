//! Native loader
use crate::message_processor::SymbolCache;
use bincode::deserialize;
#[cfg(unix)]
use libloading::os::unix::*;
#[cfg(windows)]
use libloading::os::windows::*;
use log::*;
use solana_sdk::account::KeyedAccount;
use solana_sdk::instruction::InstructionError;
use solana_sdk::instruction_processor_utils;
use solana_sdk::loader_instruction::LoaderInstruction;
use solana_sdk::pubkey::Pubkey;
use std::env;
use std::path::PathBuf;
use std::str;

/// Dynamic link library prefixes
#[cfg(unix)]
const PLATFORM_FILE_PREFIX_NATIVE: &str = "lib";
#[cfg(windows)]
const PLATFORM_FILE_PREFIX_NATIVE: &str = "";

/// Dynamic link library file extension specific to the platform
#[cfg(any(target_os = "macos", target_os = "ios"))]
const PLATFORM_FILE_EXTENSION_NATIVE: &str = "dylib";
/// Dynamic link library file extension specific to the platform
#[cfg(all(unix, not(any(target_os = "macos", target_os = "ios"))))]
const PLATFORM_FILE_EXTENSION_NATIVE: &str = "so";
/// Dynamic link library file extension specific to the platform
#[cfg(windows)]
const PLATFORM_FILE_EXTENSION_NATIVE: &str = "dll";

fn create_path(name: &str) -> PathBuf {
    let current_exe = env::current_exe()
        .unwrap_or_else(|e| panic!("create_path(\"{}\"): current exe not found: {:?}", name, e));
    let current_exe_directory = PathBuf::from(current_exe.parent().unwrap_or_else(|| {
        panic!(
            "create_path(\"{}\"): no parent directory of {:?}",
            name, current_exe,
        )
    }));

    let library_file_name = PathBuf::from(PLATFORM_FILE_PREFIX_NATIVE.to_string() + name)
        .with_extension(PLATFORM_FILE_EXTENSION_NATIVE);

    // Check the current_exe directory for the library as `cargo tests` are run
    // from the deps/ subdirectory
    let file_path = current_exe_directory.join(&library_file_name);
    if file_path.exists() {
        file_path
    } else {
        // `cargo build` places dependent libraries in the deps/ subdirectory
        current_exe_directory.join("deps").join(library_file_name)
    }
}

pub fn entrypoint(
    program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    ix_data: &[u8],
    symbol_cache: &SymbolCache,
) -> Result<(), InstructionError> {
    if keyed_accounts[0].account.executable {
        // dispatch it
        let (names, params) = keyed_accounts.split_at_mut(1);
        let name_vec = &names[0].account.data;
        if let Some(entrypoint) = symbol_cache.read().unwrap().get(name_vec) {
            unsafe {
                return entrypoint(program_id, params, ix_data);
            }
        }
        let name = match str::from_utf8(name_vec) {
            Ok(v) => v,
            Err(e) => {
                warn!("Invalid UTF-8 sequence: {}", e);
                return Err(InstructionError::GenericError);
            }
        };
        trace!("Call native {:?}", name);
        let path = create_path(&name);
        // TODO linux tls bug can cause crash on dlclose(), workaround by never unloading
        match Library::open(Some(&path), libc::RTLD_NODELETE | libc::RTLD_NOW) {
            Ok(library) => unsafe {
                let entrypoint: Symbol<instruction_processor_utils::Entrypoint> =
                    match library.get(instruction_processor_utils::ENTRYPOINT.as_bytes()) {
                        Ok(s) => s,
                        Err(e) => {
                            warn!(
                                "{:?}: Unable to find {:?} in program",
                                e,
                                instruction_processor_utils::ENTRYPOINT
                            );
                            return Err(InstructionError::GenericError);
                        }
                    };
                let ret = entrypoint(program_id, params, ix_data);
                symbol_cache
                    .write()
                    .unwrap()
                    .insert(name_vec.to_vec(), entrypoint);
                return ret;
            },
            Err(e) => {
                warn!("Unable to load: {:?}", e);
                return Err(InstructionError::GenericError);
            }
        }
    } else if let Ok(instruction) = deserialize(ix_data) {
        if keyed_accounts[0].signer_key().is_none() {
            warn!("key[0] did not sign the transaction");
            return Err(InstructionError::GenericError);
        }
        match instruction {
            LoaderInstruction::Write { offset, bytes } => {
                trace!("NativeLoader::Write offset {} bytes {:?}", offset, bytes);
                let offset = offset as usize;
                if keyed_accounts[0].account.data.len() < offset + bytes.len() {
                    warn!(
                        "Error: Overflow, {} < {}",
                        keyed_accounts[0].account.data.len(),
                        offset + bytes.len()
                    );
                    return Err(InstructionError::GenericError);
                }
                // native loader takes a name and we assume it all comes in at once
                keyed_accounts[0].account.data = bytes;
            }

            LoaderInstruction::Finalize => {
                keyed_accounts[0].account.executable = true;
                trace!(
                    "NativeLoader::Finalize prog: {:?}",
                    keyed_accounts[0].signer_key().unwrap()
                );
            }
        }
    } else {
        warn!("Invalid data in instruction: {:?}", ix_data);
        return Err(InstructionError::GenericError);
    }
    Ok(())
}
