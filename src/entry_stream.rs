//! The `entry_stream` module provides a method for streaming entries out via a
//! local unix socket, to provide client services such as a block explorer with
//! real-time access to entries.

use crate::entry::Entry;
use crate::leader_scheduler::LeaderScheduler;
use crate::result::Result;
use chrono::{SecondsFormat, Utc};
use solana_sdk::hash::Hash;
use std::io::prelude::*;
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::cell::RefCell;

pub trait Output: std::fmt::Display + std::fmt::Debug {
    fn write(&self, payload: String) -> Result<()>;
}

#[derive(Debug)]
pub struct VecOutput {
    values: RefCell<Vec<String>>,
}

impl Output for VecOutput {
    fn write(&self, payload: String) -> Result<()> {
        self.values.borrow_mut().push(payload);
        Ok(())
    }
}

impl std::fmt::Display for VecOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "VecOutput {{}}")
    }
}

impl VecOutput {
    pub fn new() -> Self {
        VecOutput { values: RefCell::new(Vec::new()) }
    }

    pub fn entries(&self) -> Vec<String> {
        self.values.borrow().clone()
    }
}

#[derive(Debug)]
pub struct SocketOutput {
    socket: String,
}

impl Output for SocketOutput {
    fn write(&self, payload: String) -> Result<()> {
        let mut socket = UnixStream::connect(Path::new(&self.socket))?;
        socket.write_all(payload.as_bytes())?;
        socket.shutdown(Shutdown::Write)?;
        Ok(())
    }
}

impl std::fmt::Display for SocketOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "VecOutput {{}}")
    }
}

pub trait EntryStreamHandler {
    fn emit_entry_events(
        &self,
        leader_scheduler: &LeaderScheduler,
        entries: &[Entry],
    ) -> Result<()>;
    fn emit_block_event(
        &self,
        leader_scheduler: &LeaderScheduler,
        tick_height: u64,
        last_id: Hash
    ) -> Result<()>;
}

#[derive(Debug)]
pub struct EntryStream<T: Output> {
    output: T,
}

impl<T> EntryStreamHandler for EntryStream<T>
where T:Output {
    fn emit_entry_events(
        &self,
        leader_scheduler: &LeaderScheduler,
        entries: &[Entry],
    ) -> Result<()> {
        for entry in entries {
            let slot = leader_scheduler
                .tick_height_to_slot(entry.tick_height);
            let leader_id = leader_scheduler
                .get_leader_for_slot(slot)
                .unwrap();

            let json_leader_id = serde_json::to_string(&leader_id)?;
            let json_entry = serde_json::to_string(&entry)?;
            let payload = format!(
                r#"{{"dt":"{}","t":"entry","s":{},"l":{},"entry":{}}}{}"#,
                Utc::now().to_rfc3339_opts(SecondsFormat::Nanos, true),
                slot,
                json_leader_id,
                json_entry,
                "\n",
            );
            self.output.write(payload)?;
        }
        Ok(())
    }

    fn emit_block_event(&self, leader_scheduler: &LeaderScheduler, tick_height: u64, last_id: Hash) -> Result<()> {
        let slot = leader_scheduler
            .tick_height_to_slot(tick_height);
        let leader_id = leader_scheduler
            .get_leader_for_slot(slot)
            .unwrap();
        let json_leader_id = serde_json::to_string(&leader_id)?;
        let json_last_id = serde_json::to_string(&last_id)?;
        let payload = format!(
            r#"{{"dt":"{}","t":"block","s":{},"h":{},"l":{},"id":{}}}{}"#,
            Utc::now().to_rfc3339_opts(SecondsFormat::Nanos, true),
            slot,
            tick_height,
            json_leader_id,
            json_last_id,
            "\n",
        );
        self.output.write(payload)?;
        Ok(())
    }
}

pub type SocketEntryStream = EntryStream<SocketOutput>;

impl SocketEntryStream {
    pub fn new(socket: String) -> Self {
        EntryStream {
            output: SocketOutput { socket },
        }
    }
}

pub type MockEntryStream = EntryStream<VecOutput>;

impl MockEntryStream {
    pub fn new(_:String) -> Self {
        EntryStream {
            output: VecOutput::new(),
        }
    }

    pub fn entries(&self) -> Vec<String> {
        self.output.entries()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::bank::Bank;
    use crate::entry::Entry;
    use crate::genesis_block::GenesisBlock;
    use crate::leader_scheduler::LeaderSchedulerConfig;
    use chrono::{DateTime, FixedOffset};
    use serde_json::Value;
    use solana_sdk::hash::Hash;
    use std::collections::HashSet;
    use std::os::unix::net::UnixListener;
    use std::result::Result::Err;
    use std::thread;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::Barrier;

    #[test]
    fn test_entry_stream() -> () {
        // Set up entry stream
        let entry_stream = MockEntryStream::new("test_stream".to_string());
        let leader_scheduler_config = LeaderSchedulerConfig::new(5, 2,10);
        let (genesis_block, _mint_keypair) = GenesisBlock::new(1_000_000);
        let bank = Bank::new_with_leader_scheduler_config(&genesis_block, &leader_scheduler_config);
        let mut leader_scheduler = bank.leader_scheduler.write().unwrap();

        let mut last_id = Hash::default();
        let mut entries = Vec::new();
        let mut expected_entries = Vec::new();

        let tick_height_initial = 0;
        let tick_height_final = tick_height_initial + leader_scheduler.ticks_per_slot + 2;
        let mut last_slot = leader_scheduler.tick_height_to_slot(tick_height_initial);

        for tick_height in tick_height_initial..=tick_height_final {
            leader_scheduler.update_tick_height(tick_height, &bank);
            let curr_slot = leader_scheduler.tick_height_to_slot(tick_height);
            if curr_slot != last_slot {
                entry_stream.emit_block_event(&leader_scheduler, tick_height - 1, last_id).unwrap_or_else(|e| {
                    error!("Entry Stream error: {:?}, {:?}", e, entry_stream);
                });
            }
            let entry = Entry::new(&mut last_id, tick_height, 1, vec![]); //just ticks
            last_id = entry.id;
            last_slot = curr_slot;
            expected_entries.push(entry.clone());
            entries.push(entry);
        }

        entry_stream
            .emit_entry_events(&leader_scheduler, &entries)
            .unwrap_or_else(|e| {
                error!("Entry Stream error: {:?}, {:?}", e, entry_stream);
            });

        assert_eq!(
            entry_stream.entries().len() as u64,
            // one entry per tick (0..=N+2) is +3, plus one block
            leader_scheduler.ticks_per_slot + 3 + 1
        );

        let mut j = 0;
        let mut matched_entries = 0;
        let mut matched_slots = HashSet::new();
        let mut matched_blocks = HashSet::new();

        for item in entry_stream.entries() {
            let json: Value = serde_json::from_str(&item).unwrap();
            let dt_str = json["dt"].as_str().unwrap();

            // Ensure `ts` field parses as valid DateTime
            let _dt: DateTime<FixedOffset> = DateTime::parse_from_rfc3339(dt_str).unwrap();

            let item_type = json["t"].as_str().unwrap();
            match item_type {
                "block" => {
                    let id = json["id"].to_string();
                    matched_blocks.insert(id);
                }

                "entry" => {
                    let slot = json["s"].as_u64().unwrap();
                    matched_slots.insert(slot);
                    let entry_obj = json["entry"].clone();
                    let entry: Entry = serde_json::from_value(entry_obj).unwrap();

                    assert_eq!(entry, expected_entries[j]);
                    matched_entries += 1;
                    j += 1;
                }

                _ => {
                    assert!(false, "unknown item type {}", item);
                }
            }
        }

        assert_eq!(matched_entries, expected_entries.len());
        assert_eq!(matched_slots.len(), 2);
        assert_eq!(matched_blocks.len(), 1);
    }

    #[test]
    fn test_vec_output() {
        let output = VecOutput::new();
        assert_eq!(output.entries().len(), 0);

        output.write("hello".to_string()).unwrap();
        output.write("byebye".to_string()).unwrap();

        let entries = output.entries();
        assert_eq!(entries[0], "hello");
        assert_eq!(entries[1], "byebye");
    }

    struct SocketCleanup {
        socket_path: String
    }

    impl Drop for SocketCleanup {
        fn drop(&mut self) {
            std::fs::remove_file(Path::new(&self.socket_path)).unwrap();
        }
    }

    impl SocketCleanup {
        fn new(socket_path: String) -> Self {
            match std::fs::remove_file(Path::new(&socket_path)) {
                _ => ()
            }

            SocketCleanup { socket_path }
        }

        fn ping(&self) {
            // do nothing
        }
    }

    #[test]
    fn test_socket_output() {
        let socket = "__test_socket_output__testsocket.sock";
        let socket_cleanup = SocketCleanup::new(socket.to_string());
        let barrier = Arc::new(Barrier::new(3));

        {
            let output= Arc::new(Mutex::new(Vec::<String>::new()));

            let reader = |x: u64, barrier: Arc<Barrier>, output : Arc<Mutex<Vec<String>>>| {
                move || {
                    println!("reader starting: {}", x);
                    let cloned_output = output.clone();
                    let n = x.clone();

                    let socket_path = Path::new(socket.clone());
                    println!("reader listening: {}", x);
                    let listener = UnixListener::bind(socket_path).unwrap();
                    println!("reader notifying: {}", x);

                    barrier.wait();
                    let mut i = 0;

                    loop {

                        println!("reader accepting: {}", x);
                        match listener.accept() {
                            Ok((mut sock, _addr)) => {
                                let mut response = String::new();
                                sock.read_to_string(&mut response).unwrap();
                                println!("reader received: {}", response.clone());
                                {
                                    cloned_output.lock().unwrap().push(response.clone());
                                }

                                i += 1;

                                if i == n {
                                    break
                                }
                            },
                            Err(e) => panic!(e)
                        }
                    }

                    println!("reader finished: {}", x);
                    socket_cleanup.ping();
                }
            };

            let writer = |x: String, barrier: Arc<Barrier>| {
                move || {
                    println!("writer starting: {}", x);


                    let y = x.clone();
                    println!("writer waiting: {}", x);
                    barrier.clone().wait();

                    println!("writer sleeping: {}", x);
//                    thread::sleep(Duration::from_millis(400));
                    println!("writer writing: {}", x);
                    let socket_output = SocketOutput { socket: socket.clone().to_string() };
                    socket_output.write(y.to_string()).unwrap();
                    println!("writer finished: {}", x);
                }
            };

            {
                assert_eq!(output.lock().unwrap().len(), 0);
            }

            let reader_thr = thread::spawn(reader(2, barrier.clone(), output.clone()));

            // give the server time to start
            let thr1 = thread::spawn(writer("hello".to_string(), barrier.clone()));
            let thr2 = thread::spawn(writer("goodbye".to_string(), barrier.clone()));

            thr1.join().unwrap();
            thr2.join().unwrap();
            reader_thr.join().unwrap();

            {
                let entries = output.lock().unwrap();
                assert!(entries.contains(&"hello".to_string()), "doesn't contain 'hello'");
                assert!(entries.contains(&"goodbye".to_string()), "doesn't contain 'goodbye'");
            }
        }
    }
}
