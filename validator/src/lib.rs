#![allow(clippy::integer_arithmetic)]
pub use solana_core::test_validator;
use {
    console::style,
    indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle},
    log::*,
    std::{env, process::exit, thread::JoinHandle},
};

pub mod admin_rpc_service;
pub mod dashboard;

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

    solana_logger::setup_with_default("solana=info");
    logger_thread
}

pub fn port_validator(port: String) -> Result<(), String> {
    port.parse::<u16>()
        .map(|_| ())
        .map_err(|e| format!("{:?}", e))
}

/// Creates a new process bar for processing that will take an unknown amount of time
pub fn new_spinner_progress_bar() -> ProgressBar {
    let progress_bar = ProgressBar::new(42);
    progress_bar.set_draw_target(ProgressDrawTarget::stdout());
    progress_bar
        .set_style(ProgressStyle::default_spinner().template("{spinner:.green} {wide_msg}"));
    progress_bar.enable_steady_tick(100);
    progress_bar
}

/// Pretty print a "name value"
pub fn println_name_value(name: &str, value: &str) {
    println!("{} {}", style(name).bold(), value);
}
