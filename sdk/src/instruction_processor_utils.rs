use crate::account::KeyedAccount;
use crate::instruction::InstructionError;
use crate::pubkey::Pubkey;

// All native programs export a symbol named process()
pub const ENTRYPOINT: &str = "process";

// Native program ENTRYPOINT prototype
pub type Entrypoint = unsafe extern "C" fn(
    program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    data: &[u8],
    tick_height: u64,
) -> Result<(), InstructionError>;

// Convenience macro to define the native program entrypoint.  Supply a fn to this macro that
// conforms to the `Entrypoint` type signature.
#[macro_export]
macro_rules! solana_entrypoint(
    ($entrypoint:ident) => (
        #[no_mangle]
        pub extern "C" fn process(
            program_id: &solana_sdk::pubkey::Pubkey,
            keyed_accounts: &mut [solana_sdk::account::KeyedAccount],
            data: &[u8],
            tick_height: u64
        ) -> Result<(), solana_sdk::instruction::InstructionError> {
            $entrypoint(program_id, keyed_accounts, data, tick_height)
        }
    )
);
