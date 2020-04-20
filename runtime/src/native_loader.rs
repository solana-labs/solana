//! Native loader
#[cfg(unix)]
use libloading::os::unix::*;
#[cfg(windows)]
use libloading::os::windows::*;
use log::*;
use num_derive::{FromPrimitive, ToPrimitive};
use solana_sdk::{
    account::KeyedAccount,
    entrypoint_native::{LoaderEntrypoint, ProgramEntrypoint},
    instruction::InstructionError,
    program_utils::{next_keyed_account, DecodeError},
    pubkey::Pubkey,
};
use std::{collections::HashMap, env, path::PathBuf, str, sync::RwLock};
use thiserror::Error;

#[derive(Error, Debug, Serialize, Clone, PartialEq, FromPrimitive, ToPrimitive)]
pub enum NativeLoaderError {
    #[error("Entrypoint name in the account data is not a valid UTF-8 string")]
    InvalidEntrypointName = 0x0aaa_0001,
    #[error("Entrypoint was not found in the module")]
    EntrypointNotFound = 0x0aaa_0002,
    #[error("Failed to load the module")]
    FailedToLoad = 0x0aaa_0003,
}
impl<T> DecodeError<T> for NativeLoaderError {
    fn type_of() -> &'static str {
        "NativeLoaderError"
    }
}

/// Dynamic link library prefixes
#[cfg(unix)]
const PLATFORM_FILE_PREFIX: &str = "lib";
#[cfg(windows)]
const PLATFORM_FILE_PREFIX: &str = "";

/// Dynamic link library file extension specific to the platform
#[cfg(any(target_os = "macos", target_os = "ios"))]
const PLATFORM_FILE_EXTENSION: &str = "dylib";
/// Dynamic link library file extension specific to the platform
#[cfg(all(unix, not(any(target_os = "macos", target_os = "ios"))))]
const PLATFORM_FILE_EXTENSION: &str = "so";
/// Dynamic link library file extension specific to the platform
#[cfg(windows)]
const PLATFORM_FILE_EXTENSION: &str = "dll";

pub type ProgramSymbolCache = RwLock<HashMap<String, Symbol<ProgramEntrypoint>>>;
pub type LoaderSymbolCache = RwLock<HashMap<String, Symbol<LoaderEntrypoint>>>;

#[derive(Debug, Default)]
pub struct NativeLoader {
    program_symbol_cache: ProgramSymbolCache,
    loader_symbol_cache: LoaderSymbolCache,
}
impl NativeLoader {
    fn create_path(name: &str) -> PathBuf {
        let current_exe = env::current_exe().unwrap_or_else(|e| {
            panic!("create_path(\"{}\"): current exe not found: {:?}", name, e)
        });
        let current_exe_directory = PathBuf::from(current_exe.parent().unwrap_or_else(|| {
            panic!(
                "create_path(\"{}\"): no parent directory of {:?}",
                name, current_exe,
            )
        }));

        let library_file_name = PathBuf::from(PLATFORM_FILE_PREFIX.to_string() + name)
            .with_extension(PLATFORM_FILE_EXTENSION);

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
    fn library_open(path: &PathBuf) -> Result<Library, libloading::Error> {
        Library::new(path)
    }

    #[cfg(not(windows))]
    fn library_open(path: &PathBuf) -> Result<Library, libloading::Error> {
        // Linux tls bug can cause crash on dlclose(), workaround by never unloading
        Library::open(Some(path), libc::RTLD_NODELETE | libc::RTLD_NOW)
    }

    fn get_entrypoint<T>(
        name: &str,
        cache: &RwLock<HashMap<String, Symbol<T>>>,
    ) -> Result<Symbol<T>, InstructionError> {
        let mut cache = cache.write().unwrap();
        if let Some(entrypoint) = cache.get(name) {
            Ok(entrypoint.clone())
        } else {
            match Self::library_open(&Self::create_path(&name)) {
                Ok(library) => {
                    let result = unsafe { library.get::<T>(name.as_bytes()) };
                    match result {
                        Ok(entrypoint) => {
                            cache.insert(name.to_string(), entrypoint.clone());
                            Ok(entrypoint)
                        }
                        Err(e) => {
                            warn!("Unable to find program entrypoint in {:?}: {:?})", name, e);
                            Err(NativeLoaderError::EntrypointNotFound.into())
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to load: {:?}", e);
                    Err(NativeLoaderError::FailedToLoad.into())
                }
            }
        }
    }

    pub fn process_instruction(
        &self,
        _program_id: &Pubkey,
        keyed_accounts: &[KeyedAccount],
        instruction_data: &[u8],
    ) -> Result<(), InstructionError> {
        let mut keyed_accounts_iter = keyed_accounts.iter();
        let program = next_keyed_account(&mut keyed_accounts_iter)?;
        let params = keyed_accounts_iter.as_slice();
        let name_vec = &program.try_account_ref()?.data;
        let name = match str::from_utf8(name_vec) {
            Ok(v) => v,
            Err(e) => {
                warn!("Invalid UTF-8 sequence: {}", e);
                return Err(NativeLoaderError::InvalidEntrypointName.into());
            }
        };
        trace!("Call native {:?}", name);
        if name.ends_with("loader_program") {
            let entrypoint =
                Self::get_entrypoint::<LoaderEntrypoint>(name, &self.loader_symbol_cache)?;
            unsafe { entrypoint(program.unsigned_key(), params, instruction_data) }
        } else {
            let entrypoint =
                Self::get_entrypoint::<ProgramEntrypoint>(name, &self.program_symbol_cache)?;
            unsafe { entrypoint(program.unsigned_key(), params, instruction_data) }
        }
    }
}
