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

// Cargo adds a hash in the filename of git and crates.io dependencies so we cannot
// deterministically build a path, we have to search for it.
fn find_library(root: PathBuf, library_name: &str) -> Option<PathBuf> {
    if let Ok(files) = root.read_dir() {
        for file in files.filter_map(Result::ok) {
            let file_path = file.path();
            if let Some(file_name) = file_path.file_name() {
                if let Some(file_name) = file_name.to_str() {
                    if file_name.ends_with(PLATFORM_FILE_EXTENSION_NATIVE)
                        && file_name.starts_with(&library_name)
                        && file_name.split(&['-', '.'][..]).next() == Some(library_name)
                    {
                        return Some(file_path);
                    }
                }
            }
        }
    }
    None
}

fn create_library_path(name: &str) -> Option<PathBuf> {
    let current_exe = env::current_exe()
        .unwrap_or_else(|e| panic!("create_path(\"{}\"): current exe not found: {:?}", name, e));
    let current_exe_directory = PathBuf::from(current_exe.parent().unwrap_or_else(|| {
        panic!(
            "create_path(\"{}\"): no parent directory of {:?}",
            name, current_exe,
        )
    }));

    let library_path = PathBuf::from(PLATFORM_FILE_PREFIX_NATIVE.to_string() + name);
    let library_name = library_path.to_str().unwrap();

    // Check the current_exe directory for the library as `cargo tests` are run
    // from the deps/ subdirectory
    find_library(current_exe_directory.clone(), library_name)
        // `cargo build` places dependent libraries in the deps/ subdirectory
        .or_else(|| find_library(current_exe_directory.join("deps"), library_name))
}

#[cfg(windows)]
fn library_open(path: &PathBuf) -> std::io::Result<Library> {
    Library::new(path)
}

#[cfg(not(windows))]
fn library_open(path: &PathBuf) -> std::io::Result<Library> {
    // TODO linux tls bug can cause crash on dlclose(), workaround by never unloading
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
    if let Some(path) = create_library_path(&name) {
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
    } else {
        warn!("Unable to find native program: {}", name);
        Err(InstructionError::GenericError)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use tempfile::tempdir;

    #[test]
    fn test_create_library_path() {
        assert!(create_library_path("solana_bpf_loader_program").is_some());
        assert!(create_library_path("solana_bpf_loader").is_none());
    }

    #[test]
    fn test_find_library() {
        let root = tempdir().unwrap();
        let prefix = "libsolana_bpf_loader_program";
        let file_path = root.path().join(format!(
            "libsolana_bpf_loader_program.{}",
            PLATFORM_FILE_EXTENSION_NATIVE
        ));
        File::create(file_path.clone()).unwrap();
        assert_eq!(
            find_library(root.path().to_path_buf(), prefix),
            Some(file_path)
        );
        assert_eq!(
            find_library(root.path().to_path_buf(), "libsolana_bpf"),
            None,
            "Library name must match completely"
        );
        assert_eq!(
            find_library(root.path().to_path_buf(), "libsolana_doesnt_exist"),
            None
        );
    }

    #[test]
    fn test_find_library_with_hash() {
        let root = tempdir().unwrap();
        let prefix = "libsolana_bpf_loader_program";
        let hashed_file_path = root.path().join(format!(
            "libsolana_bpf_loader_program-674c49511f17b07f.{}",
            PLATFORM_FILE_EXTENSION_NATIVE
        ));
        File::create(hashed_file_path.clone()).unwrap();
        assert_eq!(
            find_library(root.path().to_path_buf(), prefix),
            Some(hashed_file_path)
        );
        assert_eq!(
            find_library(root.path().to_path_buf(), "libsolana_bpf"),
            None,
            "Library name must match completely"
        );
    }
}
