extern crate elf;
extern crate rbpf;

use std::env;
use std::io::prelude::*;
use std::mem;
use std::path::PathBuf;

use bpf_verifier;
use byteorder::{LittleEndian, WriteBytesExt};
use libc;
#[cfg(unix)]
use libloading::os::unix::*;
#[cfg(windows)]
use libloading::os::windows::*;
use result::Result;

use solana_program_interface::account::KeyedAccount;
use solana_program_interface::pubkey::Pubkey;

/// Dynamic link library prefixs
const PLATFORM_FILE_PREFIX_BPF: &str = "";
#[cfg(unix)]
const PLATFORM_FILE_PREFIX_NATIVE: &str = "lib";
#[cfg(windows)]
const PLATFORM_FILE_PREFIX_NATIVE: &str = "";

/// Dynamic link library file extension specific to the platform
const PLATFORM_FILE_EXTENSION_BPF: &str = "o";
#[cfg(any(target_os = "macos", target_os = "ios"))]
const PLATFORM_FILE_EXTENSION_NATIVE: &str = "dylib";
/// Dynamic link library file extension specific to the platform
#[cfg(all(unix, not(any(target_os = "macos", target_os = "ios"))))]
const PLATFORM_FILE_EXTENSION_NATIVE: &str = "so";
/// Dynamic link library file extension specific to the platform
#[cfg(windows)]
const PLATFORM_FILE_EXTENSION_NATIVE: &str = "dll";

/// Section name
const PLATFORM_SECTION_RS: &str = ".text,entrypoint";
const PLATFORM_SECTION_C: &str = ".text.entrypoint";

pub enum ProgramPath {
    Bpf,
    Native,
}

impl ProgramPath {
    /// Creates a platform-specific file path
    pub fn create(&self, name: &str) -> PathBuf {
        let pathbuf = {
            let current_exe = env::current_exe().unwrap();
            PathBuf::from(current_exe.parent().unwrap())
        };

        pathbuf.join(match self {
            ProgramPath::Bpf => PathBuf::from(PLATFORM_FILE_PREFIX_BPF.to_string() + name)
                .with_extension(PLATFORM_FILE_EXTENSION_BPF),
            ProgramPath::Native => PathBuf::from(PLATFORM_FILE_PREFIX_NATIVE.to_string() + name)
                .with_extension(PLATFORM_FILE_EXTENSION_NATIVE),
        })
    }
}

// All programs export a symbol named process()
const ENTRYPOINT: &str = "process";
type Entrypoint = unsafe extern "C" fn(infos: &mut Vec<KeyedAccount>, data: &[u8]) -> bool;

#[derive(Debug)]
pub enum DynamicProgram {
    /// Native program
    /// * Transaction::keys[0..] - program dependent
    /// * name - name of the program, translated to a file path of the program module
    /// * userdata - program specific user data
    Native { name: String, library: Library },
    /// Bpf program
    /// * Transaction::keys[0..] - program dependent
    /// * TODO BPF specific stuff
    /// * userdata - program specific user data
    Bpf { name: String, prog: Vec<u8> },
}

impl DynamicProgram {
    pub fn new_native(name: String) -> Result<Self> {
        // create native program
        let path = ProgramPath::Native {}.create(&name);
        // TODO linux tls bug can cause crash on dlclose, workaround by never unloading
        let library = Library::open(Some(path), libc::RTLD_NODELETE | libc::RTLD_NOW)?;
        Ok(DynamicProgram::Native { name, library })
    }

    pub fn new_bpf_from_file(name: String) -> Self {
        // create native program
        let path = ProgramPath::Bpf {}.create(&name);
        let file = match elf::File::open_path(&path) {
            Ok(f) => f,
            Err(e) => panic!("Error opening ELF {:?}: {:?}", path, e),
        };

        let text_section = match file.get_section(PLATFORM_SECTION_RS) {
            Some(s) => s,
            None => match file.get_section(PLATFORM_SECTION_C) {
                Some(s) => s,
                None => panic!("Failed to find text section"),
            },
        };
        let prog = text_section.data.clone();

        DynamicProgram::Bpf { name, prog }
    }

    pub fn new_bpf_from_buffer(prog: Vec<u8>) -> Self {
        DynamicProgram::Bpf {
            name: "from_buffer".to_string(),
            prog,
        }
    }

    #[allow(dead_code)]
    fn dump_prog(name: &str, prog: &[u8]) {
        let mut eight_bytes: Vec<u8> = Vec::new();
        println!("BPF Program: {}", name);
        for i in prog.iter() {
            if eight_bytes.len() >= 7 {
                println!("{:02X?}", eight_bytes);
                eight_bytes.clear();
            } else {
                eight_bytes.push(i.clone());
            }
        }
    }

    fn serialize(infos: &mut Vec<KeyedAccount>, data: &[u8]) -> Vec<u8> {
        assert_eq!(32, mem::size_of::<Pubkey>());

        let mut v: Vec<u8> = Vec::new();
        v.write_u64::<LittleEndian>(infos.len() as u64).unwrap();
        for info in infos.iter_mut() {
            v.write_all(info.key.as_ref()).unwrap();
            v.write_i64::<LittleEndian>(info.account.tokens).unwrap();
            v.write_u64::<LittleEndian>(info.account.userdata.len() as u64)
                .unwrap();
            v.write_all(&info.account.userdata).unwrap();
            v.write_all(info.account.program_id.as_ref()).unwrap();
            //println!("userdata: {:?}", infos[i].account.userdata);
        }
        v.write_u64::<LittleEndian>(data.len() as u64).unwrap();
        v.write_all(data).unwrap();
        v
    }

    fn deserialize(infos: &mut Vec<KeyedAccount>, buffer: &[u8]) {
        assert_eq!(32, mem::size_of::<Pubkey>());

        let mut start = mem::size_of::<u64>();
        for info in infos.iter_mut() {
            start += mem::size_of::<Pubkey>() // pubkey
                  + mem::size_of::<u64>() // tokens
                  + mem::size_of::<u64>(); // length tag

            let end = start + info.account.userdata.len();

            info.account.userdata.clone_from_slice(&buffer[start..end]);

            start += info.account.userdata.len() // userdata
                  + mem::size_of::<Pubkey>(); // program_id
                                              //println!("userdata: {:?}", infos[i].account.userdata);
        }
    }

    pub fn call(&self, infos: &mut Vec<KeyedAccount>, data: &[u8]) -> bool {
        match self {
            DynamicProgram::Native { name, library } => unsafe {
                let entrypoint: Symbol<Entrypoint> = match library.get(ENTRYPOINT.as_bytes()) {
                    Ok(s) => s,
                    Err(e) => panic!(
                        "Unable to find {:?} in program {}: {:?} ",
                        e, ENTRYPOINT, name
                    ),
                };
                entrypoint(infos, data)
            },
            DynamicProgram::Bpf { prog, .. } => {
                println!("Instructions: {}", prog.len() / 8);
                //DynamicProgram::dump_prog(name, prog);

                let mut vm = rbpf::EbpfVmRaw::new(prog, Some(bpf_verifier::verifier));

                // TODO register more handlers (memcpy for example)
                vm.register_helper(
                    rbpf::helpers::BPF_TRACE_PRINTK_IDX,
                    rbpf::helpers::bpf_trace_printf,
                );

                let mut v = DynamicProgram::serialize(infos, data);
                vm.prog_exec(v.as_mut_slice());
                DynamicProgram::deserialize(infos, &v);
                true // TODO: return false on Bpf program failure
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    use solana_program_interface::account::Account;
    use solana_program_interface::pubkey::Pubkey;

    #[test]
    fn test_path_create_native() {
        let path = ProgramPath::Native {}.create("noop");
        assert_eq!(true, Path::new(&path).exists());
        let path = ProgramPath::Native {}.create("move_funds");
        assert_eq!(true, Path::new(&path).exists());
    }

    #[test]
    fn test_bpf_buf_noop() {
        let prog = vec![
            0x95, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 	exit
        ];

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

            let dp = DynamicProgram::new_bpf_from_buffer(prog);
            dp.call(&mut infos, &data);
        }
    }

    #[test]
    fn test_bpf_buf_print() {
        let prog = vec![
            0xb7, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 	r1 = 0
            0xb7, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 	r2 = 0
            0xb7, 0x03, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, // 	r3 = 1
            0xb7, 0x04, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, // 	r4 = 2
            0xb7, 0x05, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, // 	r5 = 3
            0x85, 0x00, 0x00, 0x00, 0x06, 0x00, 0x00, 0x00, // 	call 6
            0x95, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 	exit
        ];

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

            let dp = DynamicProgram::new_bpf_from_buffer(prog);
            dp.call(&mut infos, &data);
        }
    }

    // TODO add more tests to validate the Userdata and Account data is
    // moving across the boundary correctly
}
