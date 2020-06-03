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

/// Convenience macro to declare a native program
///
/// bs58_string: bs58 string representation the program's id
/// name: Name of the program, must match the library name in Cargo.toml
/// entrypoint: Program's entrypoint, must be of `type Entrypoint`
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

        #[macro_export]
        macro_rules! $name {
            () => {
                (stringify!($name).to_string(), $crate::id())
            };
        }

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

/// Same as declare_program but for native loaders
#[macro_export]
macro_rules! declare_loader(
    ($bs58_string:expr, $name:ident, $entrypoint:expr) => (
        $crate::declare_id!($bs58_string);

        #[macro_export]
        macro_rules! $name {
            () => {
                (stringify!($name).to_string(), $crate::id())
            };
        }

        #[no_mangle]
        pub extern "C" fn $name(
            program_id: &$crate::pubkey::Pubkey,
            keyed_accounts: &[$crate::account::KeyedAccount],
            instruction_data: &[u8],
            invoke_context: &mut dyn $crate::entrypoint_native::InvokeContext,
        ) -> Result<(), $crate::instruction::InstructionError> {
            $entrypoint(program_id, keyed_accounts, instruction_data, invoke_context)
        }
    )
);

pub type ProcessInstruction = fn(&Pubkey, &[KeyedAccount], &[u8]) -> Result<(), InstructionError>;

/// Cross-program invocation context passed to loaders
pub trait InvokeContext {
    fn push(&mut self, key: &Pubkey) -> Result<(), InstructionError>;
    fn pop(&mut self);
    fn verify_and_update(
        &mut self,
        message: &Message,
        instruction: &CompiledInstruction,
        accounts: &[Rc<RefCell<Account>>],
    ) -> Result<(), InstructionError>;
    fn get_caller(&self) -> Result<&Pubkey, InstructionError>;
    fn get_programs(&self) -> &[(Pubkey, ProcessInstruction)];
}
