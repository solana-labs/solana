//! The `entry_stream` module provides a method for streaming entries out via a
//! local unix socket, to provide client services such as a block explorer with
//! real-time access to entries.

use crate::entry::Entry;
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
}

impl EntryStreamHandler for EntryStream {
    fn stream_entries(&mut self, entries: &[Entry]) -> Result<()> {
        let mut socket = UnixStream::connect(Path::new(&self.socket))?;
        for entry in entries {
            let json = serde_json::to_string(&entry)?;
            let payload = format!(r#"{{"dt":"{}","entry":{}}}"#, Utc::now().to_rfc3339(), json);
            socket.write_all(payload.as_bytes())?;
        }
        socket.shutdown(Shutdown::Write)?;
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
