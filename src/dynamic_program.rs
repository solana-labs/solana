// TODO rename to native_loader

use bincode::deserialize;
use libc;
#[cfg(unix)]
use libloading::os::unix::*;
#[cfg(windows)]
use libloading::os::windows::*;
use solana_program_interface::account::KeyedAccount;
use solana_program_interface::loader_instruction::LoaderInstruction;
use solana_program_interface::pubkey::Pubkey;
use std::env;
use std::path::PathBuf;
use std::str;

/// Dynamic link library prefixs
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

/// Section name
pub const PLATFORM_SECTION_RS: &str = ".text,entrypoint";

fn create_path(name: &str) -> PathBuf {
    let pathbuf = {
        let current_exe = env::current_exe().unwrap();
        PathBuf::from(current_exe.parent().unwrap())
    };

    pathbuf.join(
        PathBuf::from(PLATFORM_FILE_PREFIX_NATIVE.to_string() + name)
            .with_extension(PLATFORM_FILE_EXTENSION_NATIVE),
    )
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum ProgramError {
    Overflow,
    UserdataDeserializeFailure,
}

pub const NATIVE_PROGRAM_ID: [u8; 32] = [2u8; 32];

// All native programs export a symbol named process()
const ENTRYPOINT: &str = "process";
type Entrypoint = unsafe extern "C" fn(keyed_accounts: &mut Vec<KeyedAccount>, data: &[u8]) -> bool;

pub enum NativeProgram {}

impl NativeProgram {
    pub fn check_id(program_id: &Pubkey) -> bool {
        program_id.as_ref() == NATIVE_PROGRAM_ID
    }

    pub fn id() -> Pubkey {
        Pubkey::new(&NATIVE_PROGRAM_ID)
    }

    pub fn process_transaction(
<<<<<<< HEAD
        tx: &Transaction,
        ix: usize,
        accounts: &mut [&mut Account],
    ) bool {
        if let Ok(instruction) = deserialize(tx.userdata(ix)) {
            trace!("program process_transaction: {:?}", instruction);
            match instruction {
                DynamicInstruction::LoadNative { name } => {
                    trace!("LoadNative: {:?}", name);
                    let dp = DynamicProgram::Native { name };
                    assert!(accounts[1].userdata.len() >= mem::size_of_val(&dp));
                    accounts[1].userdata = serialize(&dp).unwrap();
                    true
                }
                DynamicInstruction::LoadBpfFile { name } => {
                    trace!("LoadBpfFile: {:?}", name);
                    let dp = DynamicProgram::BpfFile { name };
                    assert!(accounts[1].userdata.len() >= mem::size_of_val(&dp));
                    accounts[1].userdata = serialize(&dp).unwrap();
                    true
                }
                DynamicInstruction::LoadBpf {
                    /* TODO */ offset: _offset,
                    prog,
                } => {
                    trace!("LoadBpf");
                    let dp = DynamicProgram::Bpf { prog };
                    assert!(accounts[1].userdata.len() >= mem::size_of_val(&dp));
                    accounts[1].userdata = serialize(&dp).unwrap();
                    true
                }
<<<<<<< HEAD
                DynamicInstruction::LoadState { data, .. } => {
                    // TODO handle chunks
                    println!("LoadState size {}", data.len());
                    assert!(accounts[1].userdata.len() >= data.len());
                    accounts[1].userdata = data;
                    true
                }
=======
>>>>>>> Distinguish between program and system accounts
                DynamicInstruction::Call { input } => {
                    // TODO passing account[0] (mint) and account[1] (program) as mutable
                    //      don't want to allow program to mutate those two accounts
                    let dp: DynamicProgram = deserialize(&accounts[1].userdata).unwrap();
                    match dp {
                        DynamicProgram::Native { name } => unsafe {
                            trace!("Call native {:?}", name);
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

                            trace!("Call BPF, {} Instructions", prog.len() / 8);
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
=======
        keyed_accounts: &mut Vec<KeyedAccount>,
        tx_data: &[u8],
    ) -> Result<(), ProgramError> {
        if keyed_accounts[0].account.executable {
            // dispatch it
        
            // TODO do this in a cleaner way
            let name = keyed_accounts[0].account.userdata.clone();
            let name = match str::from_utf8(&name) {
                Ok(v) => v,
                Err(e) => panic!("Invalid UTF-8 sequence: {}", e),
            };
            println!("Call native {:?}", name);
            {
                // create native program
                let path = create_path(&name);
                // TODO linux tls bug can cause crash on dlclose, workaround by never unloading
                let library =
                    Library::open(Some(path), libc::RTLD_NODELETE | libc::RTLD_NOW).unwrap();
                unsafe {
                    let entrypoint: Symbol<Entrypoint> = match library.get(ENTRYPOINT.as_bytes()) {
                        Ok(s) => s,
                        Err(e) => panic!("{:?}: Unable to find {:?} in program", e, ENTRYPOINT),
                    };
                    entrypoint(&mut keyed_accounts[1..], tx_data);
                }
            }
        } else if let Ok(instruction) = deserialize(tx_data) {
                println!("program process_transaction: {:?}", instruction);
                match instruction {
                    LoaderInstruction::Write { offset, bits } => {
                        println!("LoaderInstruction::Write offset {} bits {:?}", offset, bits);
                        let offset = offset as usize;
                        if keyed_accounts[0].account.userdata.len() <= offset + bits.len() {
                            return Err(ProgramError::Overflow);
>>>>>>> Chain loaders, BPF loader now dynamic, lua will follow
                        }
                        // native loader takes a name and we assume it all comes in at once
                        // TODO this is essentially a realloc (free and alloc)
                        keyed_accounts[0].account.userdata = bits;
                    }

<<<<<<< HEAD
                            let mut v = DynamicProgram::serialize_state(&mut infos, &input);
                            vm.prog_exec(v.as_mut_slice());
                            DynamicProgram::deserialize_state(&mut infos, &v);
                            true
                        }
=======
                    LoaderInstruction::Finalize => {
                        keyed_accounts[0].account.executable = true;
                        // TODO move this to spawn
                        keyed_accounts[0].account.loader_program_id = NativeProgram::id();
                        keyed_accounts[0].account.program_id = *keyed_accounts[0].key;
                        println!(
                            "LoaderInstruction::Finalize prog: {:?} loader {:?}",
                            keyed_accounts[0].account.program_id,
                            keyed_accounts[0].account.loader_program_id
                        );
>>>>>>> Chain loaders, BPF loader now dynamic, lua will follow
                    }
                }
            } else {
                info!("Invalid program transaction: {:?}", tx_data);
                return Err(ProgramError::UserdataDeserializeFailure)
            }
<<<<<<< HEAD
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
    use signature::{Keypair, KeypairUtil};
    use solana_program_interface::account::Account;
    use std::path::Path;

    fn dynamic_propgram_process_transaction(tx: &Transaction, accounts: &mut [Account]) {
        let mut refs: Vec<&mut Account> = accounts.iter_mut().collect();
        assert_eq!(
            Ok(()),
            DynamicProgram::process_transaction(&tx, 0, &mut refs[..])
        );
    }

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

        let mut accounts = vec![Account::default(), Account::default()];
        accounts[1].tokens = 1;
        accounts[1].userdata = serialize(&dp).unwrap();

        let instruction = DynamicInstruction::Call { input: vec![0u8] };
        let tx = Transaction::new(
            &Keypair::new(),
            &[Keypair::new().pubkey(), Keypair::new().pubkey()],
            Keypair::new().pubkey(),
            serialize(&instruction).unwrap(),
            Hash::default(),
            0,
        );

        dynamic_propgram_process_transaction(&tx, &mut accounts);
=======
        Ok(()) // TODO true
>>>>>>> Chain loaders, BPF loader now dynamic, lua will follow
    }
}
