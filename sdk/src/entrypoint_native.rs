//! Solana Native program entry point

use crate::{instruction::InstructionError, keyed_account::KeyedAccount, pubkey::Pubkey};

// Prototype of a native program entry point
///
/// program_id: Program ID of the currently executing program
/// keyed_accounts: Accounts passed as part of the instruction
/// instruction_data: Instruction data
pub type ProgramEntrypoint = unsafe extern "C" fn(
    program_id: &Pubkey,
    keyed_accounts: &[KeyedAccount],
    instruction_data: &[u8],
) -> Result<(), InstructionError>;

#[rustversion::since(1.46.0)]
#[macro_export]
macro_rules! declare_name {
    ($name:ident, $filename:ident, $id:path) => {
        #[macro_export]
        macro_rules! $name {
            () => {
                // Subtle:
                // The outer `declare_name!` macro may be expanded in another
                // crate, causing the macro `$name!` to be defined in that
                // crate. We want to emit a call to `$crate::id()`, and have
                // `$crate` be resolved in the crate where `$name!` gets defined,
                // *not* in this crate (where `declare_name! is defined).
                //
                // When a macro_rules! macro gets expanded, any $crate tokens
                // in its output will be 'marked' with the crate they were expanded
                // from. This includes nested macros like our macro `$name` - even
                // though it looks like a separate macro, Rust considers it to be
                // just another part of the output of `declare_program!`.
                //
                // We pass `$name` as the second argument to tell `respan!` to
                // apply use the `Span` of `$name` when resolving `$crate::id`.
                // This causes `$crate` to behave as though it was written
                // at the same location as the `$name` value passed
                // to `declare_name!` (e.g. the 'foo' in
                // `declare_name(foo)`
                //
                // See the `respan!` macro for more details.
                // This should use `crate::respan!` once
                // https://github.com/rust-lang/rust/pull/72121 is merged:
                // see https://github.com/solana-labs/solana/issues/10933.
                // For now, we need to use `::solana_sdk`
                //
                // `respan!` respans the path `$crate::id`, which we then call (hence the extra
                // parens)
                (
                    stringify!($filename).to_string(),
                    ::solana_sdk::respan!($crate::$id, $name)(),
                )
            };
        }
    };
}

#[rustversion::not(since(1.46.0))]
#[macro_export]
macro_rules! declare_name {
    ($name:ident, $filename:ident, $id:path) => {
        #[macro_export]
        macro_rules! $name {
            () => {
                (stringify!($filename).to_string(), $crate::$id())
            };
        }
    };
}

/// Convenience macro to declare a native program
///
/// bs58_string: bs58 string representation the program's id
/// name: Name of the program
/// filename: must match the library name in Cargo.toml
/// entrypoint: Program's entrypoint, must be of `type Entrypoint`
/// id: Path to the program id access function, used if this macro is not
///     called in `src/lib`
///
/// # Examples
///
/// ```
/// use std::str::FromStr;
/// # // wrapper is used so that the macro invocation occurs in the item position
/// # // rather than in the statement position which isn't allowed.
/// # mod item_wrapper {
/// use solana_sdk::{
///     declare_program,
///     instruction::InstructionError,
///     process_instruction::InvokeContext,
///     pubkey::Pubkey,
/// };
///
/// fn my_process_instruction(
///     first_instruction_account: usize,
///     instruction_data: &[u8],
///     invoke_context: &mut dyn InvokeContext,
/// ) -> Result<(), InstructionError> {
///   // Process an instruction
///   Ok(())
/// }
///
/// declare_program!(
///     "My11111111111111111111111111111111111111111",
///     solana_my_program,
///     my_process_instruction
/// );
///
/// # }
/// # use solana_sdk::pubkey::Pubkey;
/// # use item_wrapper::id;
/// let my_id = Pubkey::from_str("My11111111111111111111111111111111111111111").unwrap();
/// assert_eq!(id(), my_id);
/// ```
/// ```
/// use std::str::FromStr;
/// # // wrapper is used so that the macro invocation occurs in the item position
/// # // rather than in the statement position which isn't allowed.
/// # mod item_wrapper {
/// use solana_sdk::{
///     declare_program,
///     instruction::InstructionError,
///     process_instruction::InvokeContext,
///     pubkey::Pubkey,
/// };
///
/// fn my_process_instruction(
///     first_instruction_account: usize,
///     instruction_data: &[u8],
///     invoke_context: &mut dyn InvokeContext,
/// ) -> Result<(), InstructionError> {
///   // Process an instruction
///   Ok(())
/// }
///
/// declare_program!(
///     solana_sdk::system_program::ID,
///     solana_my_program,
///     my_process_instruction
/// );
/// # }
///
/// # use item_wrapper::id;
/// assert_eq!(id(), solana_sdk::system_program::ID);
/// ```
#[macro_export]
macro_rules! declare_program(
    ($bs58_string:expr, $name:ident, $entrypoint:expr) => (
        $crate::declare_id!($bs58_string);
        $crate::declare_name!($name, $name, id);

        #[no_mangle]
        pub extern "C" fn $name(
            first_instruction_account: usize,
            instruction_data: &[u8],
            invoke_context: &mut dyn $crate::process_instruction::InvokeContext,
        ) -> Result<(), $crate::instruction::InstructionError> {
            $entrypoint(first_instruction_account, instruction_data, invoke_context)
        }
    )
);
