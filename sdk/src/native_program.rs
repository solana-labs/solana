use account::KeyedAccount;

// All native programs export a symbol named process()
pub const ENTRYPOINT: &str = "process";

// Native program ENTRYPOINT prototype
pub type Entrypoint =
    unsafe extern "C" fn(keyed_accounts: &mut [KeyedAccount], data: &[u8]) -> bool;

// Convenience macro to define the native program entrypoint.  Supply a fn to this macro that
// conforms to the `Entrypoint` type signature.
#[macro_export]
macro_rules! solana_entrypoint(
    ($entrypoint:ident) => (
        #[no_mangle]
        pub extern "C" fn process(keyed_accounts: &mut [KeyedAccount], data: &[u8]) -> bool {
            return $entrypoint(keyed_accounts, data);
        }
    )
);
