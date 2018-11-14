use account::KeyedAccount;

// All native programs export a symbol named process()
pub const ENTRYPOINT: &str = "process";

// Native program ENTRYPOINT prototype
pub type Entrypoint =
    unsafe extern "C" fn(keyed_accounts: &mut [KeyedAccount], data: &[u8]) -> bool;
