use {
    solana_core::banking_trace::TimedTracedEvent,
    std::path::{Path, PathBuf},
};

pub fn process_event_files(
    event_file_paths: &[PathBuf],
    handler_fn: &mut impl FnMut(TimedTracedEvent),
) -> std::io::Result<()> {
    for event_file_path in event_file_paths {
        process_event_file(event_file_path, handler_fn)?;
    }
    Ok(())
}

fn process_event_file(
    path: impl AsRef<Path>,
    handler_fn: &mut impl FnMut(TimedTracedEvent),
) -> std::io::Result<()> {
    let data = std::fs::read(path)?;

    // Deserialize events from the buffer
    let mut offset = 0;
    while offset < data.len() {
        match bincode::deserialize::<TimedTracedEvent>(&data[offset..]) {
            Ok(event) => {
                // Update the offset to the next event
                offset += bincode::serialized_size(&event).unwrap() as usize;
                handler_fn(event);
            }
            Err(_) => {
                return Ok(()); // TODO: Return an error
            }
        }
    }

    Ok(())
}
