//! Native loader
use crate::message_processor::SymbolCache;
#[cfg(unix)]
use libloading::os::unix::*;
#[cfg(windows)]
use libloading::os::windows::*;
use log::*;
use solana_sdk::account::KeyedAccount;
use solana_sdk::instruction::InstructionError;
use solana_sdk::instruction_processor_utils;
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

#[cfg(windows)]
fn library_open(path: &PathBuf) -> std::io::Result<Library> {
    Library::new(path)
}

#[cfg(not(windows))]
fn library_open(path: &PathBuf) -> std::io::Result<Library> {
    // Linux tls bug can cause crash on dlclose(), workaround by never unloading
    Library::open(Some(path), libc::RTLD_NODELETE | libc::RTLD_NOW)
}

pub fn invoke_entrypoint(
    program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    ix_data: &[u8],
    symbol_cache: &SymbolCache,
) -> Result<(), InstructionError> {
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
    match library_open(&path) {
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
            ret
        },
        Err(e) => {
            warn!("Unable to load: {:?}", e);
            Err(InstructionError::GenericError)
        }
    }
}
