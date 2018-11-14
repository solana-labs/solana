#[macro_use]
extern crate solana_sdk;

use solana_sdk::account::KeyedAccount;

solana_entrypoint!(entrypoint);
fn entrypoint(keyed_accounts: &mut [KeyedAccount], data: &[u8]) -> bool {
    println!("noop: keyed_accounts: {:#?}", keyed_accounts);
    println!("noop: data: {:?}", data);
    true
}
