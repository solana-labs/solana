//! Stable program log messages
//!
//! The format of these log messages should not be modified to avoid breaking downstream consumers
//! of program logging
use {
    crate::{ic_logger_msg, log_collector::LogCollector},
    base64::{prelude::BASE64_STANDARD, Engine},
    itertools::Itertools,
    solana_sdk::pubkey::Pubkey,
    std::{cell::RefCell, rc::Rc},
};

/// Log a program invoke.
///
/// The general form is:
///
/// ```notrust
/// "Program <address> invoke [<depth>]"
/// ```
pub fn program_invoke(
    log_collector: &Option<Rc<RefCell<LogCollector>>>,
    program_id: &Pubkey,
    invoke_depth: usize,
) {
    ic_logger_msg!(
        log_collector,
        "Program {} invoke [{}]",
        program_id,
        invoke_depth
    );
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
pub fn program_log(log_collector: &Option<Rc<RefCell<LogCollector>>>, message: &str) {
    ic_logger_msg!(log_collector, "Program log: {}", message);
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
pub fn program_data(log_collector: &Option<Rc<RefCell<LogCollector>>>, data: &[&[u8]]) {
    ic_logger_msg!(
        log_collector,
        "Program data: {}",
        data.iter().map(|v| BASE64_STANDARD.encode(v)).join(" ")
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
pub fn program_return(
    log_collector: &Option<Rc<RefCell<LogCollector>>>,
    program_id: &Pubkey,
    data: &[u8],
) {
    ic_logger_msg!(
        log_collector,
        "Program return: {} {}",
        program_id,
        BASE64_STANDARD.encode(data)
    );
}

/// Log successful program execution.
///
/// The general form is:
///
/// ```notrust
/// "Program <address> success"
/// ```
pub fn program_success(log_collector: &Option<Rc<RefCell<LogCollector>>>, program_id: &Pubkey) {
    ic_logger_msg!(log_collector, "Program {} success", program_id);
}

/// Log program execution failure
///
/// The general form is:
///
/// ```notrust
/// "Program <address> failed: <program error details>"
/// ```
pub fn program_failure<E: std::fmt::Display>(
    log_collector: &Option<Rc<RefCell<LogCollector>>>,
    program_id: &Pubkey,
    err: &E,
) {
    ic_logger_msg!(log_collector, "Program {} failed: {}", program_id, err);
}
