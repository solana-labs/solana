pub use solana_program::entrypoint_deprecated::*;

#[macro_export]
#[deprecated(
    since = "1.4.3",
    note = "use solana_program::entrypoint::entrypoint instead"
)]
macro_rules! entrypoint_deprecated {
    ($process_instruction:ident) => {
        /// # Safety
        #[no_mangle]
        pub unsafe extern "C" fn entrypoint(input: *mut u8) -> u64 {
            let (program_id, accounts, instruction_data) =
                unsafe { $crate::entrypoint_deprecated::deserialize(input) };
            match $process_instruction(&program_id, &accounts, &instruction_data) {
                Ok(()) => $crate::entrypoint_deprecated::SUCCESS,
                Err(error) => error.into(),
            }
        }
    };
}
