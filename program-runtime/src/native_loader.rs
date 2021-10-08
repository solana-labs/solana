//! Native loader
#[cfg(unix)]
use libloading::os::unix::*;
#[cfg(windows)]
use libloading::os::windows::*;
use log::*;
use num_derive::{FromPrimitive, ToPrimitive};
use serde::Serialize;
use solana_sdk::{
    account::ReadableAccount,
    decode_error::DecodeError,
    entrypoint_native::ProgramEntrypoint,
    instruction::InstructionError,
    keyed_account::keyed_account_at_index,
    native_loader,
    process_instruction::{InvokeContext, LoaderEntrypoint},
};
use std::{
    collections::HashMap,
    env,
    path::{Path, PathBuf},
    str,
    sync::RwLock,
};
use thiserror::Error;

#[derive(Error, Debug, Serialize, Clone, PartialEq, FromPrimitive, ToPrimitive)]
pub enum NativeLoaderError {
    #[error("Entrypoint name in the account data is not a valid UTF-8 string")]
    InvalidAccountData = 0x0aaa_0001,
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
    fn create_path(name: &str) -> Result<PathBuf, InstructionError> {
        let current_exe = env::current_exe().map_err(|e| {
            error!("create_path(\"{}\"): current exe not found: {:?}", name, e);
            InstructionError::from(NativeLoaderError::EntrypointNotFound)
        })?;
        let current_exe_directory = PathBuf::from(current_exe.parent().ok_or_else(|| {
            error!(
                "create_path(\"{}\"): no parent directory of {:?}",
                name, current_exe
            );
            InstructionError::from(NativeLoaderError::FailedToLoad)
        })?);

        let library_file_name = PathBuf::from(PLATFORM_FILE_PREFIX.to_string() + name)
            .with_extension(PLATFORM_FILE_EXTENSION);

        // Check the current_exe directory for the library as `cargo tests` are run
        // from the deps/ subdirectory
        let file_path = current_exe_directory.join(&library_file_name);
        if file_path.exists() {
            Ok(file_path)
        } else {
            // `cargo build` places dependent libraries in the deps/ subdirectory
            Ok(current_exe_directory.join("deps").join(library_file_name))
        }
    }

    #[cfg(windows)]
    fn library_open(path: &Path) -> Result<Library, libloading::Error> {
        unsafe { Library::new(path) }
    }

    #[cfg(not(windows))]
    fn library_open(path: &Path) -> Result<Library, libloading::Error> {
        unsafe {
            // Linux tls bug can cause crash on dlclose(), workaround by never unloading
            Library::open(Some(path), libc::RTLD_NODELETE | libc::RTLD_NOW)
        }
    }

    fn get_entrypoint<T>(
        name: &str,
        cache: &RwLock<HashMap<String, Symbol<T>>>,
    ) -> Result<Symbol<T>, InstructionError> {
        let mut cache = cache.write().unwrap();
        if let Some(entrypoint) = cache.get(name) {
            Ok(entrypoint.clone())
        } else {
            match Self::library_open(&Self::create_path(name)?) {
                Ok(library) => {
                    let result = unsafe { library.get::<T>(name.as_bytes()) };
                    match result {
                        Ok(entrypoint) => {
                            cache.insert(name.to_string(), entrypoint.clone());
                            Ok(entrypoint)
                        }
                        Err(e) => {
                            error!("Unable to find program entrypoint in {:?}: {:?})", name, e);
                            Err(NativeLoaderError::EntrypointNotFound.into())
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to load: {:?}", e);
                    Err(NativeLoaderError::FailedToLoad.into())
                }
            }
        }
    }

    pub fn process_instruction(
        &self,
        first_instruction_account: usize,
        instruction_data: &[u8],
        invoke_context: &mut dyn InvokeContext,
    ) -> Result<(), InstructionError> {
        let (program_id, name_vec) = {
            let program_id = invoke_context.get_caller()?;
            let keyed_accounts = invoke_context.get_keyed_accounts()?;
            let program = keyed_account_at_index(keyed_accounts, first_instruction_account)?;
            if native_loader::id() != *program_id {
                error!("Program id mismatch");
                return Err(InstructionError::IncorrectProgramId);
            }
            if program.owner()? != *program_id {
                error!("Executable account now owned by loader");
                return Err(InstructionError::IncorrectProgramId);
            }
            // TODO: Remove these two copies (* deref is also a copy)
            // Both could be avoided as we know that the first KeyedAccount
            // still exists even after invoke_context.remove_first_keyed_account() is called
            (
                *program.unsigned_key(),
                &program.try_account_ref()?.data().to_vec(),
            )
        };

        let name = match str::from_utf8(name_vec) {
            Ok(v) => v,
            Err(e) => {
                error!("Invalid UTF-8 sequence: {}", e);
                return Err(NativeLoaderError::InvalidAccountData.into());
            }
        };
        if name.is_empty() || name.starts_with('\0') {
            error!("Empty name string");
            return Err(NativeLoaderError::InvalidAccountData.into());
        }
        trace!("Call native {:?}", name);
        #[allow(deprecated)]
        invoke_context.remove_first_keyed_account()?;
        if name.ends_with("loader_program") {
            let entrypoint =
                Self::get_entrypoint::<LoaderEntrypoint>(name, &self.loader_symbol_cache)?;
            unsafe { entrypoint(&program_id, instruction_data, invoke_context) }
        } else {
            let entrypoint =
                Self::get_entrypoint::<ProgramEntrypoint>(name, &self.program_symbol_cache)?;
            unsafe {
                entrypoint(
                    &program_id,
                    invoke_context.get_keyed_accounts()?,
                    instruction_data,
                )
            }
        }
    }
}
