extern crate bincode;
extern crate generic_array;

use libc;
use libloading;
use solana_program_interface::account::KeyedAccount;
use std::path::PathBuf;

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
    let mut path = PathBuf::from(env!("OUT_DIR"));
    path.pop();
    path.pop();
    path.pop();
    path.push("deps");
    path.push(PLATFORM_FILE_PREFIX.to_string() + name);
    path.set_extension(PLATFORM_FILE_EXTENSION);
    path
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
    use std::path::Path;

    #[test]
    fn test_create_library_path() {
        let path = create_library_path("noop");
        assert_eq!(true, Path::new(&path).exists());
        let path = create_library_path("print");
        assert_eq!(true, Path::new(&path).exists());
        let path = create_library_path("move_funds");
        assert_eq!(true, Path::new(&path).exists());
    }

    // TODO add more tests to validate the Userdata and Account data is
    // moving across the boundary correctly

}
