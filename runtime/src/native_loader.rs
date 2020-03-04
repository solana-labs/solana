//! Native loader
use crate::message_processor::SymbolCache;
#[cfg(unix)]
use libloading::os::unix::*;
#[cfg(windows)]
use libloading::os::windows::*;
use log::*;
use num_derive::{FromPrimitive, ToPrimitive};
use solana_sdk::{
    account::KeyedAccount,
    entrypoint_native,
    instruction::InstructionError,
    program_utils::{next_keyed_account, DecodeError},
    pubkey::Pubkey,
};
use std::{env, path::PathBuf, str};
use thiserror::Error;

#[derive(Error, Debug, Serialize, Clone, PartialEq, FromPrimitive, ToPrimitive)]
pub enum NativeLoaderError {
    #[error("Entrypoint name in the account data is not a valid UTF-8 string")]
    InvalidEntrypointName,
    #[error("Entrypoint was not found in the module")]
    EntrypointNotFound,
    #[error("Failed to load the module")]
    FailedToLoad,
}
impl<T> DecodeError<T> for NativeLoaderError {
    fn type_of() -> &'static str {
        "NativeLoaderError"
    }
}

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
    keyed_accounts: &[KeyedAccount],
    instruction_data: &[u8],
    symbol_cache: &SymbolCache,
) -> Result<(), InstructionError> {
    let mut keyed_accounts_iter = keyed_accounts.iter();
    let program = next_keyed_account(&mut keyed_accounts_iter)?;
    let params = keyed_accounts_iter.as_slice();
    let name_vec = &program.try_account_ref()?.data;
    if let Some(entrypoint) = symbol_cache.read().unwrap().get(name_vec) {
        unsafe {
            return entrypoint(program_id, params, instruction_data);
        }
    }
    let name = match str::from_utf8(name_vec) {
        Ok(v) => v,
        Err(e) => {
            warn!("Invalid UTF-8 sequence: {}", e);
            return Err(NativeLoaderError::InvalidEntrypointName.into());
        }
    };
    trace!("Call native {:?}", name);
    let path = create_path(&name);
    match library_open(&path) {
        Ok(library) => unsafe {
            let entrypoint: Symbol<entrypoint_native::Entrypoint> =
                match library.get(name.as_bytes()) {
                    Ok(s) => s,
                    Err(e) => {
                        warn!(
                            "Unable to find entrypoint {:?} (error: {:?})",
                            name.as_bytes(),
                            e
                        );
                        return Err(NativeLoaderError::EntrypointNotFound.into());
                    }
                };
            let ret = entrypoint(program_id, params, instruction_data);
            symbol_cache
                .write()
                .unwrap()
                .insert(name_vec.to_vec(), entrypoint);
            ret
        },
        Err(e) => {
            warn!("Failed to load: {:?}", e);
            Err(NativeLoaderError::FailedToLoad.into())
        }
    }
}
