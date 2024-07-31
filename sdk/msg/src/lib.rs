#[cfg(target_os = "solana")]
use solana_define_syscall::define_syscall;
/// Print a message to the log.
///
/// Supports simple strings as well as Rust [format strings][fs]. When passed a
/// single expression it will be passed directly to [`sol_log`]. The expression
/// must have type `&str`, and is typically used for logging static strings.
/// When passed something other than an expression, particularly
/// a sequence of expressions, the tokens will be passed through the
/// [`format!`] macro before being logged with `sol_log`.
///
/// [fs]: https://doc.rust-lang.org/std/fmt/
/// [`format!`]: https://doc.rust-lang.org/std/fmt/fn.format.html
///
/// Note that Rust's formatting machinery is relatively CPU-intensive
/// for constrained environments like the Solana VM.
///
/// # Examples
///
/// ```
/// use solana_msg::msg;
///
/// // The fast form
/// msg!("verifying multisig");
///
/// // With formatting
/// let err = "not enough signers";
/// msg!("multisig failed: {}", err);
/// ```
#[macro_export]
macro_rules! msg {
    ($msg:expr) => {
        $crate::sol_log($msg)
    };
    ($($arg:tt)*) => ($crate::sol_log(&format!($($arg)*)));
}

#[cfg(target_os = "solana")]
define_syscall!(fn sol_log_(message: *const u8, len: u64));

/// Print a string to the log.
#[inline]
pub fn sol_log(message: &str) {
    #[cfg(target_os = "solana")]
    unsafe {
        sol_log_(message.as_ptr(), message.len() as u64);
    }

    #[cfg(not(target_os = "solana"))]
    println!("{message}");
}
