use std::fs;
use unix_socket::UnixListener;

#[cfg(any(not(unix), target_os = "macos"))]
pub const SOLANA_SYS_TUNER_PATH: &str = "/tmp/solana-sys-tuner";

// Use abstract sockets for Unix builds
#[cfg(all(unix, target_os = "linux"))]
pub const SOLANA_SYS_TUNER_PATH: &str = "\0/tmp/solana-sys-tuner";

#[cfg(target_os = "linux")]
fn tune_system() {
    use std::process::Command;
    use std::str::from_utf8;
    use thread_priority::*;

    let output = Command::new("ps")
        .arg("-eT")
        .output()
        .expect("Expected to see all threads");

    if output.status.success() {
        if let Ok(threads) = from_utf8(&output.stdout) {
            let mut threads: Vec<&str> = threads.split('\n').collect();
            for t in threads.iter_mut() {
                if let Some(_) = t.find("solana-poh-ser") {
                    let pids: Vec<&str> = t.split_whitespace().collect();
                    let thread_id = pids[1].parse::<u64>().unwrap();
                    let _ignored = set_thread_priority(
                        thread_id,
                        ThreadPriority::Max,
                        ThreadSchedulePolicy::Realtime(RealtimeThreadSchedulePolicy::Fifo),
                    );
                    break;
                }
            }
        }
    }
}

#[cfg(any(not(unix), target_os = "macos"))]
fn tune_system() {}

fn main() {
    solana_logger::setup();
    let listener = match UnixListener::bind(SOLANA_SYS_TUNER_PATH) {
        Ok(l) => l,
        Err(_) => {
            fs::remove_file(SOLANA_SYS_TUNER_PATH).unwrap();
            UnixListener::bind(SOLANA_SYS_TUNER_PATH).unwrap()
        }
    };

    for stream in listener.incoming() {
        if stream.is_ok() {
            tune_system();
        }
    }
}
