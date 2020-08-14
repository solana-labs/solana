//! @brief Solana Native program entry point

use crate::{
    account::Account, account::KeyedAccount, instruction::CompiledInstruction,
    instruction::InstructionError, message::Message, pubkey::Pubkey,
};
use std::{cell::RefCell, rc::Rc};

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

// Prototype of a native loader entry point
///
/// program_id: Program ID of the currently executing program
/// keyed_accounts: Accounts passed as part of the instruction
/// instruction_data: Instruction data
/// invoke_context: Invocation context
pub type LoaderEntrypoint = unsafe extern "C" fn(
    program_id: &Pubkey,
    keyed_accounts: &[KeyedAccount],
    instruction_data: &[u8],
    invoke_context: &dyn InvokeContext,
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
/// use solana_sdk::account::KeyedAccount;
/// use solana_sdk::instruction::InstructionError;
/// use solana_sdk::pubkey::Pubkey;
/// use solana_sdk::declare_program;
///
/// fn my_process_instruction(
///     program_id: &Pubkey,
///     keyed_accounts: &[KeyedAccount],
///     instruction_data: &[u8],
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
/// use solana_sdk::account::KeyedAccount;
/// use solana_sdk::instruction::InstructionError;
/// use solana_sdk::pubkey::Pubkey;
/// use solana_sdk::declare_program;
///
/// fn my_process_instruction(
///     program_id: &Pubkey,
///     keyed_accounts: &[KeyedAccount],
///     instruction_data: &[u8],
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
            program_id: &$crate::pubkey::Pubkey,
            keyed_accounts: &[$crate::account::KeyedAccount],
            instruction_data: &[u8],
        ) -> Result<(), $crate::instruction::InstructionError> {
            $entrypoint(program_id, keyed_accounts, instruction_data)
        }
    )
);

pub type ProcessInstruction = fn(&Pubkey, &[KeyedAccount], &[u8]) -> Result<(), InstructionError>;
pub type ProcessInstructionWithContext =
    fn(&Pubkey, &[KeyedAccount], &[u8], &mut dyn InvokeContext) -> Result<(), InstructionError>;

/// Invocation context passed to loaders
pub trait InvokeContext {
    /// Push a program ID on to the invocation stack
    fn push(&mut self, key: &Pubkey) -> Result<(), InstructionError>;
    /// Pop a program ID off of the invocation stack
    fn pop(&mut self);
    /// Verify and update PreAccount state based on program execution
    fn verify_and_update(
        &mut self,
        message: &Message,
        instruction: &CompiledInstruction,
        accounts: &[Rc<RefCell<Account>>],
    ) -> Result<(), InstructionError>;
    /// Get the program ID of the currently executing program
    fn get_caller(&self) -> Result<&Pubkey, InstructionError>;
    /// Get a list of built-in programs
    fn get_programs(&self) -> &[(Pubkey, ProcessInstruction)];
    /// Get this invocation's logger
    fn get_logger(&self) -> Rc<RefCell<dyn Logger>>;
    /// Are cross program invocations supported
    fn is_cross_program_supported(&self) -> bool;
}

/// Log messages
pub trait Logger {
    fn log_enabled(&self) -> bool;
    /// Log a message
    fn log(&mut self, message: &str);
}
