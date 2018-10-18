pub mod bpf_verifier;

extern crate bincode;
extern crate byteorder;
extern crate elf;
extern crate env_logger;
extern crate libc;
#[macro_use]
extern crate log;
extern crate rbpf;
#[macro_use]
extern crate serde_derive;
extern crate solana_program_interface;

use bincode::{deserialize, serialize};
use byteorder::{ByteOrder, LittleEndian, WriteBytesExt};
use solana_program_interface::account::KeyedAccount;
use solana_program_interface::loader_instruction::LoaderInstruction;
use solana_program_interface::pubkey::Pubkey;
use std::env;
use std::io::prelude::*;
use std::io::Error;
use std::mem;
use std::path::PathBuf;
use std::str;
use std::sync::{Once, ONCE_INIT};

/// Dynamic link library prefixs
const PLATFORM_FILE_PREFIX_BPF: &str = "";

/// Dynamic link library file extension specific to the platform
const PLATFORM_FILE_EXTENSION_BPF: &str = "o";

/// Section name
pub const PLATFORM_SECTION_RS: &str = ".text,entrypoint";
pub const PLATFORM_SECTION_C: &str = ".text.entrypoint";

fn create_path(name: &str) -> PathBuf {
    let pathbuf = {
        let current_exe = env::current_exe().unwrap();
        PathBuf::from(current_exe.parent().unwrap().parent().unwrap())
    };

    pathbuf.join(
        PathBuf::from(PLATFORM_FILE_PREFIX_BPF.to_string() + name)
            .with_extension(PLATFORM_FILE_EXTENSION_BPF),
    )
}

fn create_vm(prog: &[u8]) -> Result<rbpf::EbpfVmRaw, Error> {
    let mut vm = rbpf::EbpfVmRaw::new(None)?;
    vm.set_verifier(bpf_verifier::check)?;
    vm.set_prog(&prog)?;
    vm.register_helper(
        rbpf::helpers::BPF_TRACE_PRINTK_IDX,
        rbpf::helpers::bpf_trace_printf,
    );
    Ok(vm)
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

fn serialize_state(keyed_accounts: &mut [KeyedAccount], data: &[u8]) -> Vec<u8> {
    assert_eq!(32, mem::size_of::<Pubkey>());

    let mut v: Vec<u8> = Vec::new();
    v.write_u64::<LittleEndian>(keyed_accounts.len() as u64)
        .unwrap();
    for info in keyed_accounts.iter_mut() {
        v.write_all(info.key.as_ref()).unwrap();
        v.write_i64::<LittleEndian>(info.account.tokens).unwrap();
        v.write_u64::<LittleEndian>(info.account.userdata.len() as u64)
            .unwrap();
        v.write_all(&info.account.userdata).unwrap();
        v.write_all(info.account.program_id.as_ref()).unwrap();
    }
    v.write_u64::<LittleEndian>(data.len() as u64).unwrap();
    v.write_all(data).unwrap();
    v
}

fn deserialize_state(keyed_accounts: &mut [KeyedAccount], buffer: &[u8]) {
    assert_eq!(32, mem::size_of::<Pubkey>());

    let mut start = mem::size_of::<u64>();
    for info in keyed_accounts.iter_mut() {
        start += mem::size_of::<Pubkey>(); // skip pubkey
        info.account.tokens = LittleEndian::read_i64(&buffer[start..]);

        start += mem::size_of::<u64>() // skip tokens
                  + mem::size_of::<u64>(); // skip length tag
        let end = start + info.account.userdata.len();
        info.account.userdata.clone_from_slice(&buffer[start..end]);

        start += info.account.userdata.len() // skip userdata
                  + mem::size_of::<Pubkey>(); // skip program_id
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum BpfLoader {
    File { name: String },
    Bytes { bytes: Vec<u8> },
}

#[no_mangle]
pub extern "C" fn process(keyed_accounts: &mut [KeyedAccount], tx_data: &[u8]) -> bool {
    static INIT: Once = ONCE_INIT;
    INIT.call_once(|| {
        // env_logger can only be initialized once
        env_logger::init();
    });

    if keyed_accounts[0].account.executable {
        let prog: Vec<u8>;
        if let Ok(program) = deserialize(&keyed_accounts[0].account.userdata) {
            match program {
                BpfLoader::File { name } => {
                    trace!("Call Bpf with file {:?}", name);
                    let path = create_path(&name);
                    let file = match elf::File::open_path(&path) {
                        Ok(f) => f,
                        Err(e) => {
                            warn!("Error opening ELF {:?}: {:?}", path, e);
                            return false;
                        }
                    };

                    let text_section = match file.get_section(PLATFORM_SECTION_RS) {
                        Some(s) => s,
                        None => match file.get_section(PLATFORM_SECTION_C) {
                            Some(s) => s,
                            None => {
                                warn!("Failed to find elf section {:?}", PLATFORM_SECTION_C);
                                return false;
                            }
                        },
                    };
                    prog = text_section.data.clone();
                }
                BpfLoader::Bytes { bytes } => {
                    trace!("Call Bpf with bytes");
                    prog = bytes;
                }
            }
        } else {
            warn!("deserialize failed: {:?}", tx_data);
            return false;
        }
        trace!("Call BPF, {} Instructions", prog.len() / 8);

        let vm = match create_vm(&prog) {
            Ok(vm) => vm,
            Err(e) => {
                warn!("{}", e);
                return false;
            }
        };

        let mut v = serialize_state(&mut keyed_accounts[1..], &tx_data);
        if 0 == vm.prog_exec(v.as_mut_slice()) {
            warn!("BPF program failed");
            return false;
        }
        deserialize_state(&mut keyed_accounts[1..], &v);
    } else if let Ok(instruction) = deserialize(tx_data) {
        match instruction {
            LoaderInstruction::Write { offset, bytes } => {
                trace!("BPFLoader::Write offset {} bytes {:?}", offset, bytes);
                let offset = offset as usize;
                if keyed_accounts[0].account.userdata.len() <= offset + bytes.len() {
                    return false;
                }
                let name = match str::from_utf8(&bytes) {
                    Ok(s) => s.to_string(),
                    Err(e) => {
                        println!("Invalid UTF-8 sequence: {}", e);
                        return false;
                    }
                };
                trace!("name: {:?}", name);
                let s = serialize(&BpfLoader::File { name }).unwrap();
                keyed_accounts[0]
                    .account
                    .userdata
                    .splice(0..s.len(), s.iter().cloned());
            }
            LoaderInstruction::Finalize => {
                keyed_accounts[0].account.executable = true;
                trace!("BPfLoader::Finalize prog: {:?}", keyed_accounts[0].key);
            }
        }
    } else {
        warn!("Invalid program transaction: {:?}", tx_data);
    }
    true
}
