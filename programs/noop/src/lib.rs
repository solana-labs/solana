extern crate solana;

use solana::dynamic_program::KeyedAccount;

#[no_mangle]
pub extern "C" fn process(_infos: &mut Vec<KeyedAccount>, _data: &[u8]) {}
