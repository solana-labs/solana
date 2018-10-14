extern crate solana_program_interface;

use solana_program_interface::account::KeyedAccount;

#[no_mangle]
pub extern "C" fn process(infos: &mut Vec<KeyedAccount>, data: &[u8]) -> bool {
    println!("noop: AccountInfos: {:#?}", infos);
    println!("noop: data: {:#?}", data);
    true
}
