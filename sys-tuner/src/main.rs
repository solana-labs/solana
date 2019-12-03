use log::*;

#[cfg(target_os = "linux")]
fn tune_system() {
    fn find_pid<P: AsRef<std::path::Path>, F>(name: &str, path: P, processor: F) -> Option<u64>
    where
        F: Fn(&std::fs::DirEntry) -> Option<u64>,
    {
        for entry in std::fs::read_dir(path).expect("Failed to read /proc folder") {
            if let Ok(dir) = entry {
                let mut path = dir.path();
                path.push("comm");
                if let Ok(comm) = std::fs::read_to_string(path.as_path()) {
                    if comm.starts_with(name) {
                        if let Some(pid) = processor(&dir) {
                            return Some(pid);
                        }
                    }
                }
            }
        }

        None
    }

    use std::process::Command;
    use std::str::from_utf8;

    if let Some(pid) = find_pid("solana-validato", "/proc", |dir| {
        let mut path = dir.path();
        path.push("task");
        find_pid("solana-poh-serv", path, |dir1| {
            if let Ok(pid) = dir1.file_name().into_string() {
                pid.parse::<u64>().ok()
            } else {
                None
            }
        })
    }) {
        info!("PoH thread PID is {}", pid);
        let pid = format!("{}", pid);
        let output = Command::new("chrt")
            .args(&["-r", "-p", "99", pid.as_str()])
            .output()
            .expect("Expected to set priority of thread");
        if output.status.success() {
            info!("Done setting thread priority");
        } else {
            error!("chrt stderr: {}", from_utf8(&output.stderr).unwrap_or("?"));
        }
    } else {
        error!("Could not find pid for PoH thread");
    }
}

#[cfg(unix)]
fn main() {
    solana_logger::setup();
    if let Err(e) = std::fs::remove_file(solana_sys_tuner::SOLANA_SYS_TUNER_PATH) {
        if e.kind() != std::io::ErrorKind::NotFound {
            panic!("Failed to remove stale socket file: {:?}", e)
        }
    }

    let listener = unix_socket::UnixListener::bind(solana_sys_tuner::SOLANA_SYS_TUNER_PATH)
        .expect("Failed to bind to the socket file");

    // set socket permission
    #[cfg(target_os = "linux")]
    {
        if let Some(user) = users::get_user_by_name("solana") {
            let uid = format!("{}", user.uid());
            info!("UID for solana is {}", uid);
            nix::unistd::chown(
                solana_sys_tuner::SOLANA_SYS_TUNER_PATH,
                Some(nix::unistd::Uid::from_raw(user.uid())),
                None,
            )
            .expect("Expected to change UID of the socket file");
        } else {
            error!("Could not find UID for solana user");
        }
    }

    info!("Waiting for tuning requests");
    for stream in listener.incoming() {
        if stream.is_ok() {
            info!("Tuning the system now");
            #[cfg(target_os = "linux")]
            tune_system();
        }
    }

    info!("exiting");
}

#[cfg(not(unix))]
fn main() {
    error!("Unsupported platform");
}
