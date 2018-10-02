extern crate solana_program_interface;

use solana_program_interface::account::KeyedAccount;

#[no_mangle]
pub extern "C" fn process(_infos: &mut Vec<KeyedAccount>, _data: &[u8]) {}
