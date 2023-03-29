/// Wrapper for `nice(3)`.
#[cfg(target_os = "linux")]
fn nice(adjustment: i8) -> Result<i8, nix::errno::Errno> {
    unsafe {
        *libc::__errno_location() = 0;
        let niceness = libc::nice(libc::c_int::from(adjustment));
        let errno = *libc::__errno_location();
        if (niceness == -1) && (errno != 0) {
            Err(errno)
        } else {
            Ok(niceness)
        }
    }
    .map(|niceness| i8::try_from(niceness).expect("Unexpected niceness value"))
    .map_err(nix::errno::from_i32)
}

/// Adds `adjustment` to the nice value of calling thread. Negative `adjustment` increases priority,
/// positive `adjustment` decreases priority. New thread inherits nice value from current thread
/// when created.
///
/// Fails on non-Linux systems for all `adjustment` values except of zero.
#[cfg(target_os = "linux")]
pub fn renice_this_thread(adjustment: i8) -> Result<(), String> {
    // On Linux, the nice value is a per-thread attribute. See `man 7 sched` for details.
    // Other systems probably should use pthread_setschedprio(), but, on Linux, thread priority
    // is fixed to zero for SCHED_OTHER threads (which is the default).
    nice(adjustment)
        .map(|_| ())
        .map_err(|err| format!("Failed to change thread's nice value: {err}"))
}

/// Adds `adjustment` to the nice value of calling thread. Negative `adjustment` increases priority,
/// positive `adjustment` decreases priority. New thread inherits nice value from current thread
/// when created.
///
/// Fails on non-Linux systems for all `adjustment` values except of zero.
#[cfg(not(target_os = "linux"))]
pub fn renice_this_thread(adjustment: i8) -> Result<(), String> {
    if adjustment == 0 {
        Ok(())
    } else {
        Err(String::from(
            "Failed to change thread's nice value: only supported on Linux",
        ))
    }
}

/// Check whether the nice value can be changed by `adjustment`.
#[cfg(target_os = "linux")]
pub fn is_renice_allowed(adjustment: i8) -> bool {
    use caps::{CapSet, Capability};

    if adjustment >= 0 {
        true
    } else {
        nix::unistd::geteuid().is_root()
            || caps::has_cap(None, CapSet::Effective, Capability::CAP_SYS_NICE)
                .map_err(|err| warn!("Failed to get thread's capabilities: {}", err))
                .unwrap_or(false)
    }
}

/// Check whether the nice value can be changed by `adjustment`.
#[cfg(not(target_os = "linux"))]
pub fn is_renice_allowed(adjustment: i8) -> bool {
    adjustment == 0
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "linux")]
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn test_nice() {
        // No change / get current niceness
        let niceness = nice(0).unwrap();

        // Decrease priority (allowed for unprivileged processes)
        let result = std::thread::spawn(|| nice(1)).join().unwrap();
        assert_eq!(result, Ok(niceness + 1));

        // Sanity check: ensure that current thread's nice value not changed after previous call
        // from different thread
        assert_eq!(nice(0), Ok(niceness));

        // Sanity check: ensure that new thread inherits nice value from current thread
        let inherited_niceness = std::thread::spawn(|| {
            nice(1).unwrap();
            std::thread::spawn(|| nice(0).unwrap()).join().unwrap()
        })
        .join()
        .unwrap();
        assert_eq!(inherited_niceness, niceness + 1);

        if !is_renice_allowed(-1) {
            // Increase priority (not allowed for unprivileged processes)
            let result = std::thread::spawn(|| nice(-1)).join().unwrap();
            assert!(result.is_err());
        }
    }
}
