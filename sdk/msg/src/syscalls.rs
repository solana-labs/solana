/// Syscall definitions used by `solana_msg`.
use solana_define_syscall::define_syscall;

define_syscall!(fn sol_log_(message: *const u8, len: u64));
define_syscall!(fn sol_log_64_(arg1: u64, arg2: u64, arg3: u64, arg4: u64, arg5: u64));
define_syscall!(fn sol_log_compute_units_());
define_syscall!(fn sol_log_data(data: *const u8, data_len: u64));
