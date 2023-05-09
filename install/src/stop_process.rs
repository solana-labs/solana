use std::{io, process::Child};

fn kill_process(process: &mut Child) -> Result<(), io::Error> {
    if let Ok(()) = process.kill() {
        process.wait()?;
    } else {
        println!("Process {} has already exited", process.id());
    }
    Ok(())
}

#[cfg(windows)]
pub fn stop_process(process: &mut Child) -> Result<(), io::Error> {
    kill_process(process)
}

#[cfg(not(windows))]
pub fn stop_process(process: &mut Child) -> Result<(), io::Error> {
    use {
        nix::{
            errno::Errno::{EINVAL, EPERM, ESRCH},
            sys::signal::{kill, Signal},
            unistd::Pid,
        },
        std::{
            io::ErrorKind,
            thread,
            time::{Duration, Instant},
        },
    };

    let nice_wait = Duration::from_secs(5);
    let pid = Pid::from_raw(process.id() as i32);
    match kill(pid, Signal::SIGINT) {
        Ok(()) => {
            let expire = Instant::now() + nice_wait;
            while let Ok(None) = process.try_wait() {
                if Instant::now() > expire {
                    break;
                }
                thread::sleep(nice_wait / 10);
            }
            if let Ok(None) = process.try_wait() {
                kill_process(process)?;
            }
        }
        Err(EINVAL) => {
            println!("Invalid signal. Killing process {pid}");
            kill_process(process)?;
        }
        Err(EPERM) => {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                format!("Insufficient permissions to signal process {pid}"),
            ));
        }
        Err(ESRCH) => {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                format!("Process {pid} does not exist"),
            ));
        }
        Err(e) => {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                format!("Unexpected error {e}"),
            ));
        }
    };
    Ok(())
}
