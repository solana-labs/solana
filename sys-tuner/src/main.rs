use clap::{crate_description, crate_name, value_t_or_exit, App, Arg};
use log::*;

#[cfg(target_os = "linux")]
#[allow(dead_code)]
fn tune_poh_service_priority(uid: u32) {
    fn find_pid<P: AsRef<std::path::Path>, F>(
        name: &str,
        path: P,
        uid: u32,
        processor: F,
    ) -> Option<u64>
    where
        F: Fn(&std::fs::DirEntry) -> Option<u64>,
    {
        for entry in std::fs::read_dir(path).expect("Failed to read /proc folder") {
            use std::os::unix::fs::MetadataExt;
            if let Ok(dir) = entry {
                if let Ok(meta) = std::fs::metadata(dir.path()) {
                    if uid == meta.uid() {
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
            }
        }

        None
    }

    use std::process::Command;
    use std::str::from_utf8;

    if let Some(pid) = find_pid("solana-validato", "/proc", uid, |dir| {
        let mut path = dir.path();
        path.push("task");
        find_pid("solana-poh-serv", path, uid, |dir1| {
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

#[cfg(target_os = "linux")]
fn tune_kernel_udp_buffers_and_vmmap() {
    use sysctl::CtlValue::String;
    use sysctl::Sysctl;
    fn sysctl_write(name: &str, value: &str) {
        if let Ok(ctl) = sysctl::Ctl::new(name) {
            info!("Old {} value {:?}", name, ctl.value());
            let ctl_value = String(value.to_string());
            match ctl.set_value(String(value.to_string())) {
                Ok(v) if v == ctl_value => info!("Updated {} to {:?}", name, ctl_value),
                Ok(v) => info!(
                    "Update returned success but {} was set to {:?}, instead of {:?}",
                    name, v, ctl_value
                ),
                Err(e) => error!("Failed to set {} to {:?}. Err {:?}", name, ctl_value, e),
            }
        } else {
            error!("Failed to find sysctl {}", name);
        }
    }

    // Reference: https://medium.com/@CameronSparr/increase-os-udp-buffers-to-improve-performance-51d167bb1360
    sysctl_write("net.core.rmem_max", "134217728");
    sysctl_write("net.core.rmem_default", "134217728");
    sysctl_write("net.core.wmem_max", "134217728");
    sysctl_write("net.core.wmem_default", "134217728");

    // increase mmap counts for many append_vecs
    sysctl_write("vm.max_map_count", "262144");
}

#[cfg(unix)]
fn main() {
    solana_logger::setup();
    let matches = App::new(crate_name!())
        .about(crate_description!())
        .version(solana_clap_utils::version!())
        .arg(
            Arg::with_name("user")
                .long("user")
                .value_name("user name")
                .takes_value(true)
                .required(true)
                .help("Username of the peer process"),
        )
        .get_matches();

    let user = value_t_or_exit!(matches, "user", String);

    info!("Tune will service requests only from user {}", user);

    unsafe { libc::umask(0o077) };
    if let Err(e) = std::fs::remove_file(solana_sys_tuner::SOLANA_SYS_TUNER_PATH) {
        if e.kind() != std::io::ErrorKind::NotFound {
            panic!("Failed to remove stale socket file: {:?}", e)
        }
    }

    let listener = unix_socket::UnixListener::bind(solana_sys_tuner::SOLANA_SYS_TUNER_PATH)
        .expect("Failed to bind to the socket file");

    let peer_uid;

    // set socket permission
    if let Some(user) = users::get_user_by_name(&user) {
        peer_uid = user.uid();
        info!("UID for solana is {}", peer_uid);
        nix::unistd::chown(
            solana_sys_tuner::SOLANA_SYS_TUNER_PATH,
            Some(nix::unistd::Uid::from_raw(peer_uid)),
            None,
        )
        .expect("Expected to change UID of the socket file");
    } else {
        panic!("Could not find UID for {:?} user", user);
    }

    info!("Waiting for tuning requests");
    for stream in listener.incoming() {
        if stream.is_ok() {
            info!("Tuning the system now");
            #[cfg(target_os = "linux")]
            {
                tune_kernel_udp_buffers_and_vmmap();
            }
        }
    }

    info!("exiting");
}

#[cfg(not(unix))]
fn main() {
    error!("Unsupported platform");
}
