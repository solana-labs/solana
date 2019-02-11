//! The `entry_stream` module provides a method for streaming entries out via a
//! local unix socket, to provide client services such as a block explorer with
//! real-time access to entries.

use crate::entry::Entry;
use crate::leader_scheduler::LeaderScheduler;
use crate::result::Result;
use chrono::{SecondsFormat, Utc};
use std::io::prelude::*;
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::{Arc, RwLock};

pub trait EntryStreamHandler {
    fn stream_entries(&mut self, entries: &[Entry]) -> Result<()>;
}

pub struct EntryStream {
    pub socket: String,
    leader_scheduler: Arc<RwLock<LeaderScheduler>>,
}

impl EntryStream {
    pub fn new(socket: String, leader_scheduler: Arc<RwLock<LeaderScheduler>>) -> Self {
        EntryStream {
            socket,
            leader_scheduler,
        }
    }
}

impl EntryStreamHandler for EntryStream {
    fn stream_entries(&mut self, entries: &[Entry]) -> Result<()> {
        let mut socket = UnixStream::connect(Path::new(&self.socket))?;
        for entry in entries {
            let json = serde_json::to_string(&entry)?;
            let (slot, slot_leader) = {
                let leader_scheduler = self.leader_scheduler.read().unwrap();
                let slot = leader_scheduler.tick_height_to_slot(entry.tick_height);
                (slot, leader_scheduler.get_leader_for_slot(slot))
            };
            let payload = format!(
                r#"{{"dt":"{}","t":"entry","s":{},"leader_id":"{:?}","entry":{}}}"#,
                Utc::now().to_rfc3339_opts(SecondsFormat::Nanos, true),
                slot,
                slot_leader,
                json
            );
            socket.write_all(payload.as_bytes())?;
        }
        socket.shutdown(Shutdown::Write)?;
        Ok(())
    }
}

pub struct MockEntryStream {
    pub socket: Vec<String>,
    leader_scheduler: Arc<RwLock<LeaderScheduler>>,
}

impl MockEntryStream {
    #[allow(clippy::needless_pass_by_value)]
    pub fn new(_socket: String, leader_scheduler: Arc<RwLock<LeaderScheduler>>) -> Self {
        MockEntryStream {
            socket: Vec::new(),
            leader_scheduler,
        }
    }
}

impl EntryStreamHandler for MockEntryStream {
    fn stream_entries(&mut self, entries: &[Entry]) -> Result<()> {
        for entry in entries {
            let json = serde_json::to_string(&entry)?;
            let (slot, slot_leader) = {
                let leader_scheduler = self.leader_scheduler.read().unwrap();
                let slot = leader_scheduler.tick_height_to_slot(entry.tick_height);
                (slot, leader_scheduler.get_leader_for_slot(slot))
            };
            let payload = format!(
                r#"{{"dt":"{}","t":"entry","s":{},"leader_id":"{:?}","entry":{}}}"#,
                Utc::now().to_rfc3339_opts(SecondsFormat::Nanos, true),
                slot,
                slot_leader,
                json
            );
            self.socket.push(payload);
        }
        Ok(())
    }
}
