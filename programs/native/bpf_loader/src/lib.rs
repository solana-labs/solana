pub mod bpf_verifier;

extern crate bincode;
extern crate byteorder;
extern crate env_logger;
#[macro_use]
extern crate log;
extern crate libc;
extern crate solana_rbpf;
#[macro_use]
extern crate solana_sdk;

use bincode::deserialize;
use byteorder::{ByteOrder, LittleEndian, WriteBytesExt};
use libc::c_char;
use solana_rbpf::EbpfVmRaw;
use solana_sdk::account::KeyedAccount;
use solana_sdk::loader_instruction::LoaderInstruction;
use solana_sdk::pubkey::Pubkey;
use std::ffi::CStr;
use std::io::prelude::*;
use std::io::{Error, ErrorKind};
use std::mem;
use std::sync::{Once, ONCE_INIT};

// TODO use rbpf's disassemble
#[allow(dead_code)]
fn dump_program(key: &Pubkey, prog: &[u8]) {
    let mut eight_bytes: Vec<u8> = Vec::new();
    info!("BPF Program: {:?}", key);
    for i in prog.iter() {
        if eight_bytes.len() >= 7 {
            info!("{:02X?}", eight_bytes);
            eight_bytes.clear();
        } else {
            eight_bytes.push(i.clone());
        }
    }
}

#[allow(unused_variables)]
pub fn helper_sol_log_verify(
    addr: u64,
    unused2: u64,
    unused3: u64,
    unused4: u64,
    unused5: u64,
    ro_regions: &[&[u8]],
    unused7: &[&[u8]],
) -> Result<(()), Error> {
    for region in ro_regions.iter() {
        if region.as_ptr() as u64 <= addr
            && addr as u64 <= region.as_ptr() as u64 + region.len() as u64
        {
            let c_buf: *const c_char = addr as *const c_char;
            let max_size = (region.as_ptr() as u64 + region.len() as u64) - addr;
            unsafe {
                for i in 0..max_size {
                    if std::ptr::read(c_buf.offset(i as isize)) == 0 {
                        return Ok(());
                    }
                }
            }
            return Err(Error::new(ErrorKind::Other, "Error, Unterminated string"));
        }
    }
    Err(Error::new(
        ErrorKind::Other,
        "Error: Load segfault, bad string pointer",
    ))
}

pub fn helper_sol_log(addr: u64, _arg2: u64, _arg3: u64, _arg4: u64, _arg5: u64) -> u64 {
    let c_buf: *const c_char = addr as *const c_char;
    let c_str: &CStr = unsafe { CStr::from_ptr(c_buf) };
    match c_str.to_str() {
        Ok(slice) => info!("sol_log: {:?}", slice),
        Err(e) => warn!("Error: Cannot print invalid string: {}", e),
    };
    0
}

pub fn helper_sol_log_u64(arg1: u64, arg2: u64, arg3: u64, arg4: u64, arg5: u64) -> u64 {
    info!(
        "sol_log_u64: {:#x}, {:#x}, {:#x}, {:#x}, {:#x}",
        arg1, arg2, arg3, arg4, arg5
    );
    0
}

pub fn create_vm(prog: &[u8]) -> Result<EbpfVmRaw, Error> {
    let mut vm = EbpfVmRaw::new(None)?;
    vm.set_verifier(bpf_verifier::check)?;
    vm.set_max_instruction_count(36000)?; // 36000 is a wag, need to tune
    vm.set_elf(&prog)?;
    vm.register_helper_ex("sol_log", Some(helper_sol_log_verify), helper_sol_log)?;
    vm.register_helper_ex("sol_log_64", None, helper_sol_log_u64)?;
    Ok(vm)
}

fn serialize_parameters(
    keyed_accounts: &mut [KeyedAccount],
    data: &[u8],
    tick_height: u64,
) -> Vec<u8> {
    assert_eq!(32, mem::size_of::<Pubkey>());

    let mut v: Vec<u8> = Vec::new();
    v.write_u64::<LittleEndian>(keyed_accounts.len() as u64)
        .unwrap();
    for info in keyed_accounts.iter_mut() {
        v.write_all(info.key.as_ref()).unwrap();
        v.write_u64::<LittleEndian>(info.account.tokens).unwrap();
        v.write_u64::<LittleEndian>(info.account.userdata.len() as u64)
            .unwrap();
        v.write_all(&info.account.userdata).unwrap();
        v.write_all(info.account.owner.as_ref()).unwrap();
    }
    v.write_u64::<LittleEndian>(data.len() as u64).unwrap();
    v.write_all(data).unwrap();
    v.write_u64::<LittleEndian>(tick_height).unwrap();
    v
}

fn deserialize_parameters(keyed_accounts: &mut [KeyedAccount], buffer: &[u8]) {
    assert_eq!(32, mem::size_of::<Pubkey>());

    let mut start = mem::size_of::<u64>();
    for info in keyed_accounts.iter_mut() {
        start += mem::size_of::<Pubkey>(); // skip pubkey
        info.account.tokens = LittleEndian::read_u64(&buffer[start..]);

        start += mem::size_of::<u64>() // skip tokens
                  + mem::size_of::<u64>(); // skip length tag
        let end = start + info.account.userdata.len();
        info.account.userdata.clone_from_slice(&buffer[start..end]);

        start += info.account.userdata.len() // skip userdata
                  + mem::size_of::<Pubkey>(); // skip owner
    }
}

solana_entrypoint!(entrypoint);
fn entrypoint(keyed_accounts: &mut [KeyedAccount], tx_data: &[u8], tick_height: u64) -> bool {
    static INIT: Once = ONCE_INIT;
    INIT.call_once(|| {
        // env_logger can only be initialized once
        env_logger::init();
    });

    if keyed_accounts[0].account.executable {
        let prog = keyed_accounts[0].account.userdata.clone();
        trace!("Call BPF, {} instructions", prog.len() / 8);
        //dump_program(keyed_accounts[0].key, &prog);
        let mut vm = match create_vm(&prog) {
            Ok(vm) => vm,
            Err(e) => {
                warn!("create_vm failed: {}", e);
                return false;
            }
        };
        let mut v = serialize_parameters(&mut keyed_accounts[1..], &tx_data, tick_height);
        match vm.execute_program(v.as_mut_slice()) {
            Ok(status) => {
                if 0 == status {
                    return false;
                }
            }
            Err(e) => {
                warn!("execute_program failed: {}", e);
                return false;
            }
        }
        deserialize_parameters(&mut keyed_accounts[1..], &v);
        trace!(
            "BPF program executed {} instructions",
            vm.get_last_instruction_count()
        );
    } else if let Ok(instruction) = deserialize(tx_data) {
        match instruction {
            LoaderInstruction::Write { offset, bytes } => {
                let offset = offset as usize;
                let len = bytes.len();
                debug!("Write: offset={} length={}", offset, len);
                if keyed_accounts[0].account.userdata.len() < offset + len {
                    warn!(
                        "Write overflow: {} < {}",
                        keyed_accounts[0].account.userdata.len(),
                        offset + len
                    );
                    return false;
                }
                keyed_accounts[0].account.userdata[offset..offset + len].copy_from_slice(&bytes);
            }
            LoaderInstruction::Finalize => {
                keyed_accounts[0].account.executable = true;
                info!("Finalize: account {:?}", keyed_accounts[0].key);
            }
        }
    } else {
        warn!("Invalid program transaction: {:?}", tx_data);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_rbpf::helpers;

    #[test]
    #[should_panic(expected = "Error: Execution exceeded maximum number of instructions")]
    fn test_non_terminating_program() {
        #[rustfmt::skip]
        let prog = &[
            0xb7, 0x06, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // r6 = 0
            0xb7, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // r1 = 0
            0xb7, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // r2 = 0
            0xb7, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // r3 = 0
            0xb7, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // r4 = 0
            0xbf, 0x65, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // r5 = r6
            0x85, 0x00, 0x00, 0x00, 0x06, 0x00, 0x00, 0x00, // call 6
            0x07, 0x06, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, // r6 + 1
            0x05, 0x00, 0xf8, 0xff, 0x00, 0x00, 0x00, 0x00, // goto -8
            0x95, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // exit
        ];
        let input = &mut [0x00];

        let mut vm = EbpfVmRaw::new(None).unwrap();
        vm.set_verifier(bpf_verifier::check).unwrap();
        vm.set_max_instruction_count(36000).unwrap(); // 36000 is a wag, need to tune
        vm.set_program(prog).unwrap();
        vm.register_helper(helpers::BPF_TRACE_PRINTK_IDX, helpers::bpf_trace_printf)
            .unwrap();
        vm.execute_program(input).unwrap();
    }
}
