use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    process::exit,
    time::SystemTime,
};

/// Get event file paths ordered by first timestamp.
pub fn get_event_file_paths(path: impl AsRef<Path>) -> Vec<PathBuf> {
    let mut event_file_paths = get_event_file_paths_unordered(path);
    event_file_paths.sort_by_key(|event_filepath| {
        read_first_timestamp(event_filepath).unwrap_or_else(|err| {
            eprintln!(
                "Error reading first timestamp from {}: {}",
                event_filepath.display(),
                err
            );
            exit(1);
        })
    });
    event_file_paths
}

fn get_event_file_paths_unordered(path: impl AsRef<Path>) -> Vec<PathBuf> {
    (0..)
        .map(|index| {
            let event_filename = if index == 0 {
                "events".to_owned()
            } else {
                format!("events.{index}")
            };
            path.as_ref().join(event_filename)
        })
        .take_while(|event_filepath| event_filepath.exists())
        .collect()
}

fn read_first_timestamp(path: impl AsRef<Path>) -> std::io::Result<SystemTime> {
    const SYSTEM_TIME_BYTES: usize = core::mem::size_of::<SystemTime>();
    let mut buffer = [0u8; SYSTEM_TIME_BYTES];

    let mut file = File::open(path)?;
    file.read_exact(&mut buffer)?;

    let system_time = bincode::deserialize(&buffer).unwrap();
    Ok(system_time)
}
