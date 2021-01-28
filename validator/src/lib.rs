pub use solana_core::test_validator;
use {
    log::*,
    std::{env, process::exit, thread::JoinHandle},
};

#[cfg(unix)]
fn redirect_stderr(filename: &str) {
    use std::{fs::OpenOptions, os::unix::io::AsRawFd};
    match OpenOptions::new()
        .write(true)
        .create(true)
        .append(true)
        .open(filename)
    {
        Ok(file) => unsafe {
            libc::dup2(file.as_raw_fd(), libc::STDERR_FILENO);
        },
        Err(err) => eprintln!("Unable to open {}: {}", filename, err),
    }
}

// Redirect stderr to a file with support for logrotate by sending a SIGUSR1 to the process.
//
// Upon success, future `log` macros and `eprintln!()` can be found in the specified log file.
pub fn redirect_stderr_to_file(logfile: Option<String>) -> Option<JoinHandle<()>> {
    // Default to RUST_BACKTRACE=1 for more informative validator logs
    if env::var_os("RUST_BACKTRACE").is_none() {
        env::set_var("RUST_BACKTRACE", "1")
    }

    let logger_thread = match logfile {
        None => None,
        Some(logfile) => {
            #[cfg(unix)]
            {
                let signals = signal_hook::iterator::Signals::new(&[signal_hook::SIGUSR1])
                    .unwrap_or_else(|err| {
                        eprintln!("Unable to register SIGUSR1 handler: {:?}", err);
                        exit(1);
                    });

                redirect_stderr(&logfile);
                Some(std::thread::spawn(move || {
                    for signal in signals.forever() {
                        info!(
                            "received SIGUSR1 ({}), reopening log file: {:?}",
                            signal, logfile
                        );
                        redirect_stderr(&logfile);
                    }
                }))
            }
            #[cfg(not(unix))]
            {
                println!("logging to a file is not supported on this platform");
                ()
            }
        }
    };

    solana_logger::setup_with_default(
        &[
            "solana=info,solana_runtime::message_processor=error", /* info logging for all solana modules */
            "rpc=trace",   /* json_rpc request/response logging */
        ]
        .join(","),
    );

    logger_thread
}

pub fn port_validator(port: String) -> Result<(), String> {
    port.parse::<u16>()
        .map(|_| ())
        .map_err(|e| format!("{:?}", e))
}
