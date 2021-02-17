use crate::retry_rpc_operation;
use log::*;
use serde::{Deserialize, Serialize};
use solana_client::rpc_client::RpcClient;
use solana_sdk::{clock::Slot, commitment_config::CommitmentConfig, epoch_info::EpochInfo};
use std::{
    cell::RefCell,
    fs::{self, File, OpenOptions},
    io,
    ops::Range,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Default)]
struct Entry {
    slots: Range<Slot>,
    path: PathBuf,
}

impl Entry {
    pub fn new<P: AsRef<Path>>(base_path: P, slots: Range<Slot>) -> Self {
        let file_name = format!("{}-{}.json", slots.start, slots.end);
        let path = base_path.as_ref().join(file_name);
        Self { slots, path }
    }

    fn parse_filename<F: AsRef<Path>>(filename: F) -> Option<Range<Slot>> {
        let filename = filename.as_ref();
        let slot_range = filename.file_stem();
        let extension = filename.extension();
        extension
            .zip(slot_range)
            .and_then(|(extension, slot_range)| {
                if extension == "json" {
                    slot_range.to_str()
                } else {
                    None
                }
            })
            .and_then(|slot_range| {
                let mut parts = slot_range.splitn(2, '-');
                let start = parts.next().and_then(|p| p.parse::<Slot>().ok());
                let end = parts.next().and_then(|p| p.parse::<Slot>().ok());
                start.zip(end).map(|(start, end)| start..end)
            })
    }

    pub fn from_pathbuf(path: PathBuf) -> Option<Self> {
        path.file_name()
            .and_then(|n| n.to_str())
            .and_then(Self::parse_filename)
            .map(|slots| Self { slots, path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

const CACHE_VERSION: u64 = 0;
const DEFAULT_SLOTS_PER_ENTRY: u64 = 2500;
const CONFIG_FILENAME: &str = "config.yaml";

#[derive(Debug, Deserialize, Serialize)]
struct Config {
    version: u64,
    slots_per_chunk: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: CACHE_VERSION,
            slots_per_chunk: DEFAULT_SLOTS_PER_ENTRY,
        }
    }
}

pub struct ConfirmedBlockCache {
    rpc_client: RpcClient,
    base_path: PathBuf,
    entries: RefCell<Vec<Entry>>,
    config: Config,
}

impl ConfirmedBlockCache {
    fn store_config<P: AsRef<Path>>(config_path: P, config: &Config) -> io::Result<()> {
        let config_path = config_path.as_ref();
        let file = File::create(config_path)?;
        serde_yaml::to_writer(file, config).map_err(|e| {
            io::Error::new(
                io::ErrorKind::Other,
                format!(
                    "error: cannot store config `{}`: {:?}",
                    config_path.to_string_lossy(),
                    e,
                ),
            )
        })
    }

    fn load_config<P: AsRef<Path>>(config_path: P) -> io::Result<Config> {
        let config_path = config_path.as_ref();
        let file = File::open(config_path)?;
        serde_yaml::from_reader(file).map_err(|e| {
            io::Error::new(
                io::ErrorKind::Other,
                format!(
                    "error: cannot load config `{}`: {:?}",
                    config_path.to_string_lossy(),
                    e,
                ),
            )
        })
    }

    pub fn open<P: AsRef<Path>, U: AsRef<str>>(path: P, rpc_url: U) -> io::Result<Self> {
        let path = path.as_ref();
        let config_path = path.join(CONFIG_FILENAME);
        let rpc_url = rpc_url.as_ref();
        let (config, entries) = match fs::read_dir(path) {
            Ok(dir_entries) => {
                let config = Self::load_config(&config_path)?;
                if config.version != CACHE_VERSION {
                    return Err(io::Error::new(
                        io::ErrorKind::Other,
                        "unexpected cache version",
                    ));
                }
                let mut entries = dir_entries
                    .filter_map(|de| Entry::from_pathbuf(de.unwrap().path()))
                    .collect::<Vec<_>>();
                entries.sort_by(|l, r| l.slots.start.cmp(&r.slots.start));
                Ok((config, entries))
            }
            Err(err) => {
                if err.kind() == io::ErrorKind::NotFound {
                    let config = Config::default();
                    fs::create_dir_all(path)?;
                    Self::store_config(config_path, &config)?;
                    Ok((config, Vec::new()))
                } else {
                    Err(err)
                }
            }
        }?;
        Ok(Self {
            rpc_client: RpcClient::new(rpc_url.to_string()),
            base_path: path.to_path_buf(),
            entries: RefCell::new(entries),
            config,
        })
    }

    fn lookup(&self, start: Slot) -> Option<Entry> {
        let entries = self.entries.borrow();
        for i in entries.iter() {
            if i.slots.start == start {
                debug!("HIT: {}", start);
                return Some(i.clone());
            }
        }
        debug!("MISS: {}", start);
        None
    }

    fn fetch(&self, start: Slot, end: Slot, epoch_info: &EpochInfo) -> io::Result<Vec<Slot>> {
        debug!("fetching slot range: {}..{}", start, end);
        // Fingers crossed we hit the same RPC backend...
        let slots = retry_rpc_operation(42, || {
            self.rpc_client.get_confirmed_blocks(start, Some(end))
        })
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("{:?}", e)))?;

        // Only cache complete chunks
        if end + self.config.slots_per_chunk < epoch_info.absolute_slot {
            debug!("committing entry for slots {}..{}", start, end);
            let entry = Entry::new(&self.base_path, start..end);
            let file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(entry.path())?;
            serde_json::to_writer(file, &slots)?;

            self.entries.borrow_mut().push(entry);
        }

        Ok(slots)
    }

    pub fn query(&self, start: Slot, end: Slot) -> io::Result<Vec<Slot>> {
        let chunk_size = self.config.slots_per_chunk;
        let mut chunk_start = (start / chunk_size) * chunk_size;
        let mut slots = Vec::new();
        let epoch_info = self
            .rpc_client
            .get_epoch_info_with_commitment(CommitmentConfig::finalized())
            .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("{:?}", e)))?;
        let last_slot = end.min(epoch_info.absolute_slot);
        while chunk_start < last_slot {
            let mut chunk_slots = if let Some(entry) = self.lookup(chunk_start) {
                let file = File::open(entry.path())?;
                serde_json::from_reader(file)?
            } else {
                let chunk_end = chunk_start + chunk_size - 1;
                self.fetch(chunk_start, chunk_end, &epoch_info)?
            };
            slots.append(&mut chunk_slots);
            chunk_start += chunk_size;
        }
        let slots = slots
            .drain(..)
            .skip_while(|s| *s < start)
            .take_while(|s| *s <= end)
            .collect::<Vec<_>>();
        Ok(slots)
    }
}
