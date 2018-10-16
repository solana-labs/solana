extern crate solana_program_interface;

use solana_program_interface::account::KeyedAccount;

#[no_mangle]
pub extern "C" fn process(keyed_accounts: &mut [KeyedAccount], data: &[u8]) -> bool {
    println!("noop: keyed_accounts: {:#?}", keyed_accounts);
    println!("noop: data: {:?}", data);
    true
}
