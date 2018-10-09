// fn loaded_contract(
//     &self,
//     tx_program_id: &Pubkey,
//     tx: &Transaction,
//     program_index: usize,
//     accounts: &mut [&mut Account],
// ) -> bool {
//     let loaded_contracts = self.loaded_contracts.write().unwrap();
//     match loaded_contracts.get(&tx_program_id) {
//         Some(dc) => {
//             let mut infos: Vec<_> = (&tx.account_keys)
//                 .into_iter()
//                 .zip(accounts)
//                 .map(|(key, account)| KeyedAccount { key, account })
//                 .collect();

//             dc.call(&mut infos, tx.userdata(program_index));
//             true
//         }
//         None => false,
//     }
// }

extern crate elf;
extern crate rbpf;

use std::env;
use std::io::prelude::*;
use std::mem;
use std::path::PathBuf;

use bincode::{deserialize, serialize};
use bpf_verifier;
use byteorder::{ByteOrder, LittleEndian, WriteBytesExt};
use libc;
#[cfg(unix)]
use libloading::os::unix::*;
#[cfg(windows)]
use libloading::os::windows::*;
use result::Result;

use dynamic_instruction::Instruction;
use solana_program_interface::account::{Account, KeyedAccount};
use solana_program_interface::pubkey::Pubkey;
use transaction::Transaction;

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
pub const PLATFORM_SECTION_RS: &str = ".text,entrypoint";
pub const PLATFORM_SECTION_C: &str = ".text.entrypoint";

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

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum ProgramError {
    // TODO
    UserdataDeserializeFailure,
}

pub const DYNAMIC_PROGRAM_ID: [u8; 32] = [2u8; 32];

// All programs export a symbol named process()
const ENTRYPOINT: &str = "process";
type Entrypoint = unsafe extern "C" fn(infos: &mut Vec<KeyedAccount>, data: &[u8]) -> bool;

#[derive(Debug, Serialize, Deserialize)]
pub enum DynamicProgram {
    /// Native program by name
    Native { name: String },
    /// Bpf program by name
    BpfFile { name: String },
    /// Bpf program by account
    Bpf { prog: Vec<u8> },
}

impl DynamicProgram {
    pub fn check_id(program_id: &Pubkey) -> bool {
        program_id.as_ref() == DYNAMIC_PROGRAM_ID
    }

    pub fn id() -> Pubkey {
        Pubkey::new(&DYNAMIC_PROGRAM_ID)
    }

    pub fn get_balance(account: &Account) -> i64 {
        account.tokens
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

    fn serialize_state(infos: &mut Vec<KeyedAccount>, data: &[u8]) -> Vec<u8> {
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

    fn deserialize_state(infos: &mut Vec<KeyedAccount>, buffer: &[u8]) {
        assert_eq!(32, mem::size_of::<Pubkey>());

        let mut start = mem::size_of::<u64>();
        for info in infos.iter_mut() {
            start += mem::size_of::<Pubkey>(); // skip pubkey
            info.account.tokens = LittleEndian::read_i64(&buffer[start..]);

            start += mem::size_of::<u64>() // skip tokens
                  + mem::size_of::<u64>(); // skip length tag
            let end = start + info.account.userdata.len();
            info.account.userdata.clone_from_slice(&buffer[start..end]);

            start += info.account.userdata.len() // skip userdata
                  + mem::size_of::<Pubkey>(); // skip program_id
                                              //println!("userdata: {:?}", infos[i].account.userdata);
        }
    }

    pub fn process_transaction(
        tx: &Transaction,
        ix: usize,
        accounts: &mut [&mut Account],
    ) bool {
        if let Ok(instruction) = deserialize(tx.userdata(ix)) {
            trace!("program process_transaction: {:?}", instruction);
            match instruction {
                Instruction::LoadNative { name } => {
                    println!("LoadNative: {:?}", name);

                    let dp = DynamicProgram::Native { name };
                    println!("num accounts: {}", accounts.len());
                    println!(
                        "size needed {} allocated {}",
                        mem::size_of_val(&dp),
                        accounts[1].userdata.len()
                    );
                    assert!(accounts[1].userdata.len() >= mem::size_of_val(&dp));
                    accounts[1].userdata = serialize(&dp).unwrap();
                    true
                }
                Instruction::LoadBpfFile { name } => {
                    println!("LoadBpfFile: {:?}", name);

                    let dp = DynamicProgram::BpfFile { name };
                    println!("num accounts: {}", accounts.len());
                    println!(
                        "size needed {} allocated {}",
                        mem::size_of_val(&dp),
                        accounts[1].userdata.len()
                    );
                    assert!(accounts[1].userdata.len() >= mem::size_of_val(&dp));
                    accounts[1].userdata = serialize(&dp).unwrap();
                    true
                }
                Instruction::LoadBpf { offset, prog } => {
                    println!("LoadBpf");

                    let dp = DynamicProgram::Bpf { prog };
                    println!("num accounts: {}", accounts.len());
                    println!(
                        "size needed {} allocated {}",
                        mem::size_of_val(&dp),
                        accounts[1].userdata.len()
                    );
                    assert!(accounts[1].userdata.len() >= mem::size_of_val(&dp));
                    accounts[1].userdata = serialize(&dp).unwrap();
                    true
                }
                Instruction::LoadState { data, .. } => {
                    // TODO handle chunks
                    println!("LoadState size {}", data.len());
                    assert!(accounts[1].userdata.len() >= data.len());
                    accounts[1].userdata = data;
                    true
                }
                Instruction::Call { input } => {
                    // TODO passing account[0] (mint) and account[1] (program) as mutable
                    //      don't want to allow program to mutate those two accounts
                    let dp: DynamicProgram = deserialize(&accounts[1].userdata).unwrap();
                    match dp {
                        DynamicProgram::Native { name } => unsafe {
                            println!("Call native {:?}", name);
                            // create native program
                            let path = ProgramPath::Native {}.create(&name);
                            // TODO linux tls bug can cause crash on dlclose, workaround by never unloading
                            let library =
                                Library::open(Some(path), libc::RTLD_NODELETE | libc::RTLD_NOW)
                                    .unwrap();
                            let entrypoint: Symbol<Entrypoint> =
                                match library.get(ENTRYPOINT.as_bytes()) {
                                    Ok(s) => s,
                                    Err(e) => panic!(
                                        "Unable to find {:?} in program {}: {:?} ",
                                        e, ENTRYPOINT, name
                                    ),
                                };

                            let mut infos: Vec<_> = (&tx.account_keys)
                                .into_iter()
                                .zip(accounts)
                                .map(|(key, account)| KeyedAccount { key, account })
                                .collect();

                            entrypoint(&mut infos, &input);
                            true
                        },
                        DynamicProgram::BpfFile { name } => {
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

                            println!("Call BPF, {} Instructions", prog.len() / 8);
                            //DynamicProgram::dump_prog(name, prog);

                            let mut vm = rbpf::EbpfVmRaw::new(&prog, Some(bpf_verifier::verifier));

                            // TODO register more handlers (e.g: signals, memcpy, etc...)
                            vm.register_helper(
                                rbpf::helpers::BPF_TRACE_PRINTK_IDX,
                                rbpf::helpers::bpf_trace_printf,
                            );

                            let mut infos: Vec<_> = (&tx.account_keys)
                                .into_iter()
                                .zip(accounts)
                                .map(|(key, account)| KeyedAccount { key, account })
                                .collect();

                            let mut v = DynamicProgram::serialize_state(&mut infos, &input);
                            vm.prog_exec(v.as_mut_slice());
                            DynamicProgram::deserialize_state(&mut infos, &v);
                            true
                        }
                        DynamicProgram::Bpf { prog } => {
                            println!("Call BPF, {} Instructions", prog.len() / 8);
                            //DynamicProgram::dump_prog(name, prog);

                            let mut vm = rbpf::EbpfVmRaw::new(&prog, Some(bpf_verifier::verifier));

                            // TODO register more handlers (e.g: signals, memcpy, etc...)
                            vm.register_helper(
                                rbpf::helpers::BPF_TRACE_PRINTK_IDX,
                                rbpf::helpers::bpf_trace_printf,
                            );

                            let mut infos: Vec<_> = (&tx.account_keys)
                                .into_iter()
                                .zip(accounts)
                                .map(|(key, account)| KeyedAccount { key, account })
                                .collect();

                            let mut v = DynamicProgram::serialize_state(&mut infos, &input);
                            vm.prog_exec(v.as_mut_slice());
                            DynamicProgram::deserialize_state(&mut infos, &v);
                            true
                        }
                    }
                }
            }
        } else {
            info!(
                "Invalid program transaction userdata: {:?}",
                tx.userdata(ix)
            );
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hash::Hash;
    use signature::{Keypair, KeypairUtil, Signature};
    use solana_program_interface::account::Account;
    use solana_program_interface::pubkey::Pubkey;
    use std::path::Path;

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

        let dp = DynamicProgram::Bpf { prog };

        let tx = Transaction::new(
            &Keypair::new(),
            &[Keypair::new().pubkey()],
            Keypair::new().pubkey(),
            vec![0u8],
            Hash::default(),
            0,
        );

        let mut account = Account::default();
        account.tokens = 1;
        account.userdata = serialize(&dp).unwrap();

        DynamicProgram::process_transaction(&tx, 0, &mut [&mut account]);
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
        let dp = DynamicProgram::Bpf { prog };

        let tx = Transaction::new(
            &Keypair::new(),
            &[Keypair::new().pubkey()],
            Keypair::new().pubkey(),
            vec![0u8],
            Hash::default(),
            0,
        );

        let mut account = Account::default();
        account.tokens = 1;
        account.userdata = serialize(&dp).unwrap();

        DynamicProgram::process_transaction(&tx, 0, &mut [&mut account]);
    }

    // TODO add more tests to validate the Userdata and Account data is
    // moving across the boundary correctly
}
