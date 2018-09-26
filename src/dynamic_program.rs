extern crate bincode;
extern crate generic_array;

use account::Account;
use libc;
use libloading;
use pubkey::Pubkey;
use std::path::PathBuf;

#[cfg(debug_assertions)]
const CARGO_PROFILE: &str = "debug";

#[cfg(not(debug_assertions))]
const CARGO_PROFILE: &str = "release";

/// Dynamic link library prefix
#[cfg(unix)]
const PLATFORM_FILE_PREFIX: &str = "lib";
/// Dynamic link library prefix
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

/// Creates a platform-specific file path
fn create_library_path(name: &str) -> PathBuf {
    let mut path = PathBuf::new();
    path.push("target");
    path.push(CARGO_PROFILE);
    path.push("deps");
    path.push(PLATFORM_FILE_PREFIX.to_string() + name);
    path.set_extension(PLATFORM_FILE_EXTENSION);
    path
}

#[derive(Debug)]
pub struct KeyedAccount<'a> {
    pub key: &'a Pubkey,
    pub account: &'a mut Account,
}

// All programs export a symbol named process()
const ENTRYPOINT: &str = "process";
type Entrypoint = unsafe extern "C" fn(infos: &mut Vec<KeyedAccount>, data: &[u8]);

#[derive(Debug)]
pub enum DynamicProgram {
    /// Native program
    /// * Transaction::keys[0..] - program dependent
    /// * name - name of the program, translated to a file path of the program module
    /// * userdata - program specific user data
    Native {
        name: String,
        library: libloading::Library,
    },
    /// Bpf program
    /// * Transaction::keys[0..] - program dependent
    /// * TODO BPF specific stuff
    /// * userdata - program specific user data
    Bpf { userdata: Vec<u8> },
}

impl DynamicProgram {
    pub fn new(name: String) -> Self {
        // TODO determine what kind of module to load

        // create native program
        let path = create_library_path(&name);
        // TODO linux tls bug can cause crash on dlclose, workaround by never unloading
        let os_lib =
            libloading::os::unix::Library::open(Some(path), libc::RTLD_NODELETE | libc::RTLD_NOW)
                .unwrap();
        let library = libloading::Library::from(os_lib);
        DynamicProgram::Native { name, library }
    }

    pub fn call(&self, infos: &mut Vec<KeyedAccount>, data: &[u8]) {
        match self {
            DynamicProgram::Native { name, library } => unsafe {
                let entrypoint: libloading::Symbol<Entrypoint> =
                    match library.get(ENTRYPOINT.as_bytes()) {
                        Ok(s) => s,
                        Err(e) => panic!(
                            "{:?} Unable to find {:?} in program {}",
                            e, ENTRYPOINT, name
                        ),
                    };
                entrypoint(infos, data);
            },
            DynamicProgram::Bpf { .. } => {
                // TODO BPF
                println!{"Bpf program not supported"}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use account::Account;
    use bincode::serialize;
    use pubkey::Pubkey;
    use std::path::Path;
    use std::thread;

    #[test]
    fn test_create_library_path() {
        let path = create_library_path("noop");
        assert_eq!(true, Path::new(&path).exists());
        let path = create_library_path("print");
        assert_eq!(true, Path::new(&path).exists());
        let path = create_library_path("move_funds");
        assert_eq!(true, Path::new(&path).exists());
    }

    #[test]
    fn test_program_noop() {
        let data: Vec<u8> = vec![0];
        let keys = vec![Pubkey::default(); 2];
        let mut accounts = vec![Account::default(), Account::default()];
        accounts[0].tokens = 100;
        accounts[1].tokens = 1;

        {
            let mut infos: Vec<_> = (&keys)
                .into_iter()
                .zip(&mut accounts)
                .map(|(key, account)| KeyedAccount { key, account })
                .collect();

            let dp = DynamicProgram::new("noop".to_string());
            dp.call(&mut infos, &data);
        }
    }

    #[test]
    #[ignore]
    fn test_program_print() {
        let data: Vec<u8> = vec![0];
        let keys = vec![Pubkey::default(); 2];
        let mut accounts = vec![Account::default(), Account::default()];
        accounts[0].tokens = 100;
        accounts[1].tokens = 1;

        {
            let mut infos: Vec<_> = (&keys)
                .into_iter()
                .zip(&mut accounts)
                .map(|(key, account)| KeyedAccount { key, account })
                .collect();

            let dp = DynamicProgram::new("print".to_string());
            dp.call(&mut infos, &data);
        }
    }

    #[test]
    fn test_program_move_funds_success() {
        let tokens: i64 = 100;
        let data: Vec<u8> = serialize(&tokens).unwrap();
        let keys = vec![Pubkey::default(); 2];
        let mut accounts = vec![Account::default(), Account::default()];
        accounts[0].tokens = 100;
        accounts[1].tokens = 1;

        {
            let mut infos: Vec<_> = (&keys)
                .into_iter()
                .zip(&mut accounts)
                .map(|(key, account)| KeyedAccount { key, account })
                .collect();

            let dp = DynamicProgram::new("move_funds".to_string());
            dp.call(&mut infos, &data);
        }
        assert_eq!(0, accounts[0].tokens);
        assert_eq!(101, accounts[1].tokens);
    }

    #[test]
    fn test_program_move_funds_insufficient_funds() {
        let tokens: i64 = 100;
        let data: Vec<u8> = serialize(&tokens).unwrap();
        let keys = vec![Pubkey::default(); 2];
        let mut accounts = vec![Account::default(), Account::default()];
        accounts[0].tokens = 10;
        accounts[1].tokens = 1;

        {
            let mut infos: Vec<_> = (&keys)
                .into_iter()
                .zip(&mut accounts)
                .map(|(key, account)| KeyedAccount { key, account })
                .collect();

            let dp = DynamicProgram::new("move_funds".to_string());
            dp.call(&mut infos, &data);
        }
        assert_eq!(10, accounts[0].tokens);
        assert_eq!(1, accounts[1].tokens);
    }

    #[test]
    fn test_program_move_funds_succes_many_threads() {
        let num_threads = 42; // number of threads to spawn
        let num_iters = 100; // number of iterations of test in each thread
        let mut threads = Vec::new();
        for _t in 0..num_threads {
            threads.push(thread::spawn(move || {
                for _i in 0..num_iters {
                    {
                        let tokens: i64 = 100;
                        let data: Vec<u8> = serialize(&tokens).unwrap();
                        let keys = vec![Pubkey::default(); 2];
                        let mut accounts = vec![Account::default(), Account::default()];
                        accounts[0].tokens = 100;
                        accounts[1].tokens = 1;

                        {
                            let mut infos: Vec<_> = (&keys)
                                .into_iter()
                                .zip(&mut accounts)
                                .map(|(key, account)| KeyedAccount { key, account })
                                .collect();

                            let dp = DynamicProgram::new("move_funds".to_string());
                            dp.call(&mut infos, &data);
                        }
                        assert_eq!(0, accounts[0].tokens);
                        assert_eq!(101, accounts[1].tokens);
                    }
                }
            }));
        }

        for thread in threads {
            thread.join().unwrap();
        }
    }

    // TODO add more tests to validate the Userdata and Account data is
    // moving across the boundary correctly

}
