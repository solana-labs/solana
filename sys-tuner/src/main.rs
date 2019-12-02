use log::*;
use std::fs;
use unix_socket::UnixListener;

#[cfg(any(not(unix), target_os = "macos"))]
pub const SOLANA_SYS_TUNER_PATH: &str = "/tmp/solana-sys-tuner";

// Use abstract sockets for Unix builds
#[cfg(all(unix, target_os = "linux"))]
pub const SOLANA_SYS_TUNER_PATH: &str = "\0/tmp/solana-sys-tuner";

#[allow(dead_code)]
#[cfg(target_os = "linux")]
fn tune_system() {
    use std::process::Command;
    use std::str::from_utf8;

    let output = Command::new("ps")
        .arg("-eT")
        .output()
        .expect("Expected to see all threads");

    if output.status.success() {
        if let Ok(threads) = from_utf8(&output.stdout) {
            let mut threads: Vec<&str> = threads.split('\n').collect();
            for t in threads.iter_mut() {
                if t.find("solana-poh-ser").is_some() {
                    let pids: Vec<&str> = t.split_whitespace().collect();
                    let thread_id = pids[1].parse::<u64>().unwrap();
                    info!("Thread ID is {}", thread_id);
                    let output = Command::new("chrt")
                        .args(&["-r", "-p", "99", pids[1]])
                        .output()
                        .expect("Expected to set priority of thread");
                    if output.status.success() {
                        info!("Done setting thread priority");
                    } else {
                        error!("chrt stderr: {}", from_utf8(&output.stderr).unwrap_or("?"));
                    }
                    break;
                }
            }
        }
    } else {
        error!("ps stderr: {}", from_utf8(&output.stderr).unwrap_or("?"));
    }
}

#[allow(dead_code)]
#[cfg(any(not(unix), target_os = "macos"))]
fn tune_system() {}

#[allow(dead_code)]
fn main() {
    solana_logger::setup();
    let listener = match UnixListener::bind(SOLANA_SYS_TUNER_PATH) {
        Ok(l) => l,
        Err(_) => {
            fs::remove_file(SOLANA_SYS_TUNER_PATH).expect("Failed to remove stale socket file");
            UnixListener::bind(SOLANA_SYS_TUNER_PATH).expect("Failed to bind to the socket file")
        }
    };

    info!("Waiting for requests");
    for stream in listener.incoming() {
        if stream.is_ok() {
            info!("Tuning the system now");
            tune_system();
        }
    }

    info!("exiting");
}
