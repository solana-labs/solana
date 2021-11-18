//! Stable program log messages
//!
//! The format of these log messages should not be modified to avoid breaking downstream consumers
//! of program logging
use crate::{ic_logger_msg, invoke_context::Logger};
use itertools::Itertools;
use solana_sdk::{instruction::InstructionError, pubkey::Pubkey};
use std::{cell::RefCell, rc::Rc};

/// Log a program invoke.
///
/// The general form is:
///
/// ```notrust
/// "Program <address> invoke [<depth>]"
/// ```
pub fn program_invoke(logger: &Rc<RefCell<dyn Logger>>, program_id: &Pubkey, invoke_depth: usize) {
    ic_logger_msg!(logger, "Program {} invoke [{}]", program_id, invoke_depth);
}

/// Log a message from the program itself.
///
/// The general form is:
///
/// ```notrust
/// "Program log: <program-generated output>"
/// ```
///
/// That is, any program-generated output is guaranteed to be prefixed by "Program log: "
pub fn program_log(logger: &Rc<RefCell<dyn Logger>>, message: &str) {
    ic_logger_msg!(logger, "Program log: {}", message);
}

/// Emit a program data.
///
/// The general form is:
///
/// ```notrust
/// "Program data: <binary-data-in-base64>*"
/// ```
///
/// That is, any program-generated output is guaranteed to be prefixed by "Program data: "
pub fn program_data(logger: &Rc<RefCell<dyn Logger>>, data: &[&[u8]]) {
    ic_logger_msg!(
        logger,
        "Program data: {}",
        data.iter().map(base64::encode).join(" ")
    );
}

/// Log return data as from the program itself. This line will not be present if no return
/// data was set, or if the return data was set to zero length.
///
/// The general form is:
///
/// ```notrust
/// "Program return: <program-id> <program-generated-data-in-base64>"
/// ```
///
/// That is, any program-generated output is guaranteed to be prefixed by "Program return: "
pub fn program_return(logger: &Rc<RefCell<dyn Logger>>, program_id: &Pubkey, data: &[u8]) {
    ic_logger_msg!(
        logger,
        "Program return: {} {}",
        program_id,
        base64::encode(data)
    );
}

/// Log successful program execution.
///
/// The general form is:
///
/// ```notrust
/// "Program <address> success"
/// ```
pub fn program_success(logger: &Rc<RefCell<dyn Logger>>, program_id: &Pubkey) {
    ic_logger_msg!(logger, "Program {} success", program_id);
}

/// Log program execution failure
///
/// The general form is:
///
/// ```notrust
/// "Program <address> failed: <program error details>"
/// ```
pub fn program_failure(
    logger: &Rc<RefCell<dyn Logger>>,
    program_id: &Pubkey,
    err: &InstructionError,
) {
    ic_logger_msg!(logger, "Program {} failed: {}", program_id, err);
}
