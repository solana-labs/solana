pub use solana_core::test_validator;
use {
    log::*,
    serde_derive::{Deserialize, Serialize},
    std::{
        env,
        fs::{self, File},
        io::{self, Write},
        net::SocketAddr,
        path::Path,
        process::exit,
        thread::JoinHandle,
        time::{Duration, SystemTime},
    },
};

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

#[derive(Serialize, Deserialize, Clone, Debug)]
struct ProcessInfo {
    rpc_addr: Option<SocketAddr>, // RPC port to contact the validator at
    start_time: u64,              // Seconds since the UNIX_EPOCH for when the validator was started
}

pub fn record_start(ledger_path: &Path, rpc_addr: Option<&SocketAddr>) -> Result<(), io::Error> {
    if !ledger_path.exists() {
        fs::create_dir_all(&ledger_path)?;
    }

    let start_info = ProcessInfo {
        rpc_addr: rpc_addr.cloned(),
        start_time: SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    };

    let serialized = serde_yaml::to_string(&start_info)
        .map_err(|err| io::Error::new(io::ErrorKind::Other, format!("{:?}", err)))?;

    let mut file = File::create(ledger_path.join("process-info.yml"))?;
    file.write_all(&serialized.into_bytes())?;
    Ok(())
}

fn get_validator_process_info(
    ledger_path: &Path,
) -> Result<(Option<SocketAddr>, SystemTime), io::Error> {
    let file = File::open(ledger_path.join("process-info.yml"))?;
    let config: ProcessInfo = serde_yaml::from_reader(file)
        .map_err(|err| io::Error::new(io::ErrorKind::Other, format!("{:?}", err)))?;

    let start_time = SystemTime::UNIX_EPOCH + Duration::from_secs(config.start_time);
    Ok((config.rpc_addr, start_time))
}

pub fn get_validator_rpc_addr(ledger_path: &Path) -> Result<Option<SocketAddr>, io::Error> {
    get_validator_process_info(ledger_path).map(|process_info| process_info.0)
}

pub fn get_validator_start_time(ledger_path: &Path) -> Result<SystemTime, io::Error> {
    get_validator_process_info(ledger_path).map(|process_info| process_info.1)
}
