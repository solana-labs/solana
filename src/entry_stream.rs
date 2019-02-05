//! The `entry_stream` module provides a method for streaming entries out via a
//! local unix socket, to provide client services such as a block explorer with
//! real-time access to entries.

use crate::entry::Entry;
use crate::leader_scheduler::DEFAULT_TICKS_PER_SLOT;
use crate::result::Result;
use chrono::Utc;
use std::io::prelude::*;
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::Path;

pub trait EntryStreamHandler {
    fn stream_entries(&mut self, entries: &[Entry]) -> Result<()>;
}

pub struct EntryStream {
    pub socket: String,
}

impl EntryStream {
    pub fn new(socket: String) -> Self {
        EntryStream { socket }
    }

    fn socket_write(&mut self, payload: &str) -> Result<()> {
        let mut socket = UnixStream::connect(Path::new(&self.socket))?;
        socket.write_all(payload.as_bytes())?;
        socket.shutdown(Shutdown::Write)?;

        Ok(())
    }
}

impl EntryStreamHandler for EntryStream {
    fn stream_entries(&mut self, entries: &[Entry]) -> Result<()> {
        for entry in entries {
            let json = serde_json::to_string(&entry)?;
            let payload = format!(
                r#"{{"dt":"{}","entry":{}}}{}"#,
                Utc::now().to_rfc3339(),
                json,
                "\n",
            );
            self.socket_write(&payload)?;

            // on block boundary, emit "block" event which signals that a block is closed
            // TODO: look at this in the context of forks, where the tuple (tick_height, entry.id)
            // might not necessarily map 1:1 to block_id
            if ((entry.tick_height + 1) % DEFAULT_TICKS_PER_SLOT) == 0 {
                let json = serde_json::to_string(&entry.id)?;
                let payload = format!(
                    r#"{{"dt":"{}","block":{{"id":{},"tick_height":{},"count":{},"block_height":{}}}}}{}"#,
                    Utc::now().to_rfc3339(),
                    json,
                    entry.tick_height + 1 - DEFAULT_TICKS_PER_SLOT,
                    DEFAULT_TICKS_PER_SLOT,
                    (entry.tick_height + 1) / DEFAULT_TICKS_PER_SLOT,
                    "\n",
                );
                self.socket_write(&payload)?;
            }
        }

        Ok(())
    }
}

pub struct MockEntryStream {
    pub socket: Vec<String>,
}

impl MockEntryStream {
    #[allow(clippy::needless_pass_by_value)]
    pub fn new(_socket: String) -> Self {
        MockEntryStream { socket: Vec::new() }
    }
}

impl EntryStreamHandler for MockEntryStream {
    fn stream_entries(&mut self, entries: &[Entry]) -> Result<()> {
        for entry in entries {
            let json = serde_json::to_string(&entry)?;
            let payload = format!(r#"{{"dt":"{}","entry":{}}}"#, Utc::now().to_rfc3339(), json);
            self.socket.push(payload);
        }
        Ok(())
    }
}
