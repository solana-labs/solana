pub mod bpf_verifier;

extern crate bincode;
extern crate byteorder;
extern crate env_logger;
#[macro_use]
extern crate log;
extern crate rbpf;
extern crate solana_program_interface;

use bincode::deserialize;
use byteorder::{ByteOrder, LittleEndian, WriteBytesExt};
use solana_program_interface::account::KeyedAccount;
use solana_program_interface::loader_instruction::LoaderInstruction;
use solana_program_interface::pubkey::Pubkey;
use std::io::prelude::*;
use std::io::Error;
use std::mem;
use std::str;
use std::sync::{Once, ONCE_INIT};

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
fn dump_prog(key: &Pubkey, prog: &[u8]) {
    let mut eight_bytes: Vec<u8> = Vec::new();
    trace!("BPF Program: {:?}", key);
    for i in prog.iter() {
        if eight_bytes.len() >= 7 {
            println!("{:02X?}", eight_bytes);
            eight_bytes.clear();
        } else {
            eight_bytes.push(i.clone());
        }
    }
}

fn serialize_parameters(keyed_accounts: &mut [KeyedAccount], data: &[u8]) -> Vec<u8> {
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

fn deserialize_parameters(keyed_accounts: &mut [KeyedAccount], buffer: &[u8]) {
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

#[no_mangle]
pub extern "C" fn process(keyed_accounts: &mut [KeyedAccount], tx_data: &[u8]) -> bool {
    static INIT: Once = ONCE_INIT;
    INIT.call_once(|| {
        // env_logger can only be initialized once
        env_logger::init();
    });

    if keyed_accounts[0].account.executable {
        let prog = keyed_accounts[0].account.userdata.clone();
        trace!("Call BPF, {} Instructions", prog.len() / 8);
        // dump_prog(keyed_accounts[0].key, &prog);
        let vm = match create_vm(&prog) {
            Ok(vm) => vm,
            Err(e) => {
                warn!("{}", e);
                return false;
            }
        };
        let mut v = serialize_parameters(&mut keyed_accounts[1..], &tx_data);
        if 0 == vm.prog_exec(v.as_mut_slice()) {
            warn!("BPF program failed");
            return false;
        }
        deserialize_parameters(&mut keyed_accounts[1..], &v);
    } else if let Ok(instruction) = deserialize(tx_data) {
        match instruction {
            LoaderInstruction::Write { offset, bytes } => {
                let offset = offset as usize;
                let len = bytes.len();
                trace!("BpfLoader::Write offset {} length {:?}", offset, len);
                if keyed_accounts[0].account.userdata.len() < offset + len {
                    println!(
                        "Overflow {} < {}",
                        keyed_accounts[0].account.userdata.len(),
                        offset + len
                    );
                    return false;
                }
                keyed_accounts[0].account.userdata[offset..offset + len].copy_from_slice(&bytes);
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
