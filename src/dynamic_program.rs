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
type Entrypoint = unsafe extern "C" fn(keyed_accounts: &mut [KeyedAccount], data: &[u8]) -> bool;

pub enum NativeProgram {}

impl NativeProgram {
    pub fn check_id(program_id: &Pubkey) -> bool {
        program_id.as_ref() == NATIVE_PROGRAM_ID
    }

    pub fn id() -> Pubkey {
        Pubkey::new(&NATIVE_PROGRAM_ID)
    }

    pub fn process_transaction(
        keyed_accounts: &mut Vec<KeyedAccount>,
        tx_data: &[u8],
    ) -> bool {
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
                            return false;
                        }
                        // native loader takes a name and we assume it all comes in at once
                        // TODO this is essentially a realloc (free and alloc)
                        keyed_accounts[0].account.userdata = bits;
                    }

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
                    }
                }
            } else {
                info!("Invalid program transaction: {:?}", tx_data);
                return false;
            }
        true
    }
}