//! The Rust-based BPF program entrypoint supported by the original BPF loader.
//!
//! The original BPF loader is deprecated and exists for backwards-compatibility
//! reasons. This module should not be used by new programs.
//!
//! For more information see the [`bpf_loader_deprecated`] module.
//!
//! [`bpf_loader_deprecated`]: crate::bpf_loader_deprecated

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
