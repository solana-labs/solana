//! Provides interfaces for rebuilding snapshot storages

use {
    super::{get_io_error, snapshot_version_from_file, SnapshotError, SnapshotVersion},
    crate::{
        account_storage::AccountStorageMap,
        accounts_db::{AccountStorageEntry, AppendVecId, AtomicAppendVecId},
        serde_snapshot::{
            self, remap_and_reconstruct_single_storage, snapshot_storage_lengths_from_fields,
            SerdeStyle, SerializedAppendVecId,
        },
    },
    crossbeam_channel::{select, unbounded, Receiver, Sender},
    dashmap::DashMap,
    log::*,
    rayon::{
        iter::{IntoParallelIterator, ParallelIterator},
        ThreadPool, ThreadPoolBuilder,
    },
    regex::Regex,
    solana_sdk::clock::Slot,
    std::{
        collections::HashMap,
        fs::File,
        io::BufReader,
        path::PathBuf,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc, Mutex,
        },
        time::Instant,
    },
};

lazy_static! {
    static ref VERSION_FILE_REGEX: Regex = Regex::new(r"^version$").unwrap();
    static ref BANK_FIELDS_FILE_REGEX: Regex = Regex::new(r"^[0-9]+(\.pre)?$").unwrap();
    static ref STORAGE_FILE_REGEX: Regex =
        Regex::new(r"^(?P<slot>[0-9]+)\.(?P<id>[0-9]+)$").unwrap();
}

/// Convenient wrapper for snapshot version and rebuilt storages
pub(crate) struct RebuiltSnapshotStorage {
    /// Snapshot version
    pub snapshot_version: SnapshotVersion,
    /// Rebuilt storages
    pub storage: AccountStorageMap,
}

/// Stores state for rebuilding snapshot storages
#[derive(Debug)]
pub(crate) struct SnapshotStorageRebuilder {
    /// Receiver for unpacked snapshot storage files
    file_receiver: Receiver<PathBuf>,
    /// Number of threads to rebuild with
    num_threads: usize,
    /// Snapshot storage lengths - from the snapshot file
    snapshot_storage_lengths: HashMap<Slot, HashMap<SerializedAppendVecId, usize>>,
    /// Container for storing snapshot file paths
    storage_paths: DashMap<Slot, Mutex<Vec<PathBuf>>>,
    /// Container for storing rebuilt snapshot storages
    storage: AccountStorageMap,
    /// Tracks next append_vec_id
    next_append_vec_id: Arc<AtomicAppendVecId>,
    /// Tracker for number of processed slots
    processed_slot_count: AtomicUsize,
    /// Tracks the number of collisions in AppendVecId
    num_collisions: AtomicUsize,
}

impl SnapshotStorageRebuilder {
    /// Synchronously spawns threads to rebuild snapshot storages
    pub(crate) fn rebuild_storage(
        file_receiver: Receiver<PathBuf>,
        num_threads: usize,
        next_append_vec_id: Arc<AtomicAppendVecId>,
    ) -> Result<RebuiltSnapshotStorage, SnapshotError> {
        let (snapshot_version_path, snapshot_file_path, append_vec_files) =
            Self::get_version_and_snapshot_files(&file_receiver);
        let snapshot_version_str = snapshot_version_from_file(snapshot_version_path)?;
        let snapshot_version = snapshot_version_str.parse().map_err(|_| {
            get_io_error(&format!(
                "unsupported snapshot version: {snapshot_version_str}",
            ))
        })?;
        let snapshot_storage_lengths =
            Self::process_snapshot_file(snapshot_version, snapshot_file_path)?;

        let account_storage_map = Self::spawn_rebuilder_threads(
            file_receiver,
            num_threads,
            next_append_vec_id,
            snapshot_storage_lengths,
            append_vec_files,
        )
        .map_err(|err| SnapshotError::IoWithSource(err, "rebuild snapshot storages"))?;

        Ok(RebuiltSnapshotStorage {
            snapshot_version,
            storage: account_storage_map,
        })
    }

    /// Create the SnapshotStorageRebuilder for storing state during rebuilding
    ///     - pre-allocates data for storage paths
    fn new(
        file_receiver: Receiver<PathBuf>,
        num_threads: usize,
        next_append_vec_id: Arc<AtomicAppendVecId>,
        snapshot_storage_lengths: HashMap<Slot, HashMap<usize, usize>>,
    ) -> Self {
        let storage = DashMap::with_capacity(snapshot_storage_lengths.len());
        let storage_paths: DashMap<_, _> = snapshot_storage_lengths
            .iter()
            .map(|(slot, storage_lengths)| {
                (*slot, Mutex::new(Vec::with_capacity(storage_lengths.len())))
            })
            .collect();
        Self {
            file_receiver,
            num_threads,
            snapshot_storage_lengths,
            storage_paths,
            storage,
            next_append_vec_id,
            processed_slot_count: AtomicUsize::new(0),
            num_collisions: AtomicUsize::new(0),
        }
    }

    /// Waits for snapshot file
    /// Due to parallel unpacking, we may receive some append_vec files before the snapshot file
    /// This function will push append_vec files into a buffer until we receive the snapshot file
    fn get_version_and_snapshot_files(
        file_receiver: &Receiver<PathBuf>,
    ) -> (PathBuf, PathBuf, Vec<PathBuf>) {
        let mut append_vec_files = Vec::with_capacity(1024);
        let mut snapshot_version_path = None;
        let mut snapshot_file_path = None;

        loop {
            if let Ok(path) = file_receiver.recv() {
                let filename = path.file_name().unwrap().to_str().unwrap();
                match get_snapshot_file_kind(filename) {
                    Some(SnapshotFileKind::Version) => {
                        snapshot_version_path = Some(path);

                        // break if we have both the snapshot file and the version file
                        if snapshot_file_path.is_some() {
                            break;
                        }
                    }
                    Some(SnapshotFileKind::BankFields) => {
                        snapshot_file_path = Some(path);

                        // break if we have both the snapshot file and the version file
                        if snapshot_version_path.is_some() {
                            break;
                        }
                    }
                    Some(SnapshotFileKind::Storage) => {
                        append_vec_files.push(path);
                    }
                    None => {} // do nothing for other kinds of files
                }
            } else {
                panic!("did not receive snapshot file from unpacking threads");
            }
        }
        let snapshot_version_path = snapshot_version_path.unwrap();
        let snapshot_file_path = snapshot_file_path.unwrap();

        (snapshot_version_path, snapshot_file_path, append_vec_files)
    }

    /// Process the snapshot file to get the size of each snapshot storage file
    fn process_snapshot_file(
        snapshot_version: SnapshotVersion,
        snapshot_file_path: PathBuf,
    ) -> Result<HashMap<Slot, HashMap<usize, usize>>, bincode::Error> {
        let snapshot_file = File::open(snapshot_file_path).unwrap();
        let mut snapshot_stream = BufReader::new(snapshot_file);
        match snapshot_version {
            SnapshotVersion::V1_2_0 => {
                let (_bank_fields, accounts_fields) =
                    serde_snapshot::fields_from_stream(SerdeStyle::Newer, &mut snapshot_stream)?;

                Ok(snapshot_storage_lengths_from_fields(&accounts_fields))
            }
        }
    }

    /// Spawn threads for processing buffered append_vec_files, and then received files
    fn spawn_rebuilder_threads(
        file_receiver: Receiver<PathBuf>,
        num_threads: usize,
        next_append_vec_id: Arc<AtomicAppendVecId>,
        snapshot_storage_lengths: HashMap<Slot, HashMap<usize, usize>>,
        append_vec_files: Vec<PathBuf>,
    ) -> Result<AccountStorageMap, std::io::Error> {
        let rebuilder = Arc::new(SnapshotStorageRebuilder::new(
            file_receiver,
            num_threads,
            next_append_vec_id,
            snapshot_storage_lengths,
        ));

        let thread_pool = rebuilder.build_thread_pool();

        // Synchronously process buffered append_vec_files
        thread_pool.install(|| rebuilder.process_buffered_files(append_vec_files))?;

        // Asynchronously spawn threads to process received append_vec_files
        let (exit_sender, exit_receiver) = unbounded();
        for _ in 0..rebuilder.num_threads {
            Self::spawn_receiver_thread(&thread_pool, exit_sender.clone(), rebuilder.clone());
        }
        drop(exit_sender); // drop otherwise loop below will never end

        // wait for asynchronous threads to complete
        rebuilder.wait_for_completion(exit_receiver)?;
        Ok(Arc::try_unwrap(rebuilder).unwrap().storage)
    }

    /// Processes buffered append_vec_files
    fn process_buffered_files(&self, append_vec_files: Vec<PathBuf>) -> Result<(), std::io::Error> {
        append_vec_files
            .into_par_iter()
            .map(|path| self.process_append_vec_file(path))
            .collect::<Result<(), std::io::Error>>()
    }

    /// Spawn a single thread to process received append_vec_files
    fn spawn_receiver_thread(
        thread_pool: &ThreadPool,
        exit_sender: Sender<Result<(), std::io::Error>>,
        rebuilder: Arc<SnapshotStorageRebuilder>,
    ) {
        thread_pool.spawn(move || {
            for path in rebuilder.file_receiver.iter() {
                match rebuilder.process_append_vec_file(path) {
                    Ok(_) => {}
                    Err(err) => {
                        exit_sender
                            .send(Err(err))
                            .expect("sender should be connected");
                        return;
                    }
                }
            }

            exit_sender
                .send(Ok(()))
                .expect("sender should be connected");
        })
    }

    /// Process an append_vec_file
    fn process_append_vec_file(&self, path: PathBuf) -> Result<(), std::io::Error> {
        let filename = path.file_name().unwrap().to_str().unwrap().to_owned();
        if let Some(SnapshotFileKind::Storage) = get_snapshot_file_kind(&filename) {
            let (slot, slot_complete) = self.insert_slot_storage_file(path, filename);
            if slot_complete {
                self.process_complete_slot(slot)?;
                self.processed_slot_count.fetch_add(1, Ordering::AcqRel);
            }
        }
        Ok(())
    }

    /// Inserts single storage file, returns the slot and if the slot has all of its storage entries
    fn insert_slot_storage_file(&self, path: PathBuf, filename: String) -> (Slot, bool) {
        let (slot, _) = get_slot_and_append_vec_id(&filename);
        let slot_storage_count = self.insert_storage_file(&slot, path);

        (
            slot,
            slot_storage_count == self.snapshot_storage_lengths.get(&slot).unwrap().len(),
        )
    }

    /// Insert storage path into slot and return the number of storage files for the slot
    fn insert_storage_file(&self, slot: &Slot, path: PathBuf) -> usize {
        let slot_paths = self.storage_paths.get(slot).unwrap();
        let mut lock = slot_paths.lock().unwrap();
        lock.push(path);
        lock.len()
    }

    /// Process a slot that has received all storage entries
    fn process_complete_slot(&self, slot: Slot) -> Result<(), std::io::Error> {
        let slot_storage_paths = self.storage_paths.get(&slot).unwrap();
        let lock = slot_storage_paths.lock().unwrap();

        let slot_stores = lock
            .iter()
            .map(|path| {
                let filename = path.file_name().unwrap().to_str().unwrap();
                let (_, old_append_vec_id) = get_slot_and_append_vec_id(filename);
                let current_len = *self
                    .snapshot_storage_lengths
                    .get(&slot)
                    .unwrap()
                    .get(&old_append_vec_id)
                    .unwrap();

                let storage_entry = remap_and_reconstruct_single_storage(
                    slot,
                    old_append_vec_id,
                    current_len,
                    path.as_path(),
                    &self.next_append_vec_id,
                    &self.num_collisions,
                )?;

                Ok((storage_entry.append_vec_id(), storage_entry))
            })
            .collect::<Result<HashMap<AppendVecId, Arc<AccountStorageEntry>>, std::io::Error>>()?;

        let slot_entry = self.storage.entry(slot).or_default();
        let mut storage_lock = slot_entry.write().unwrap();
        *storage_lock = slot_stores;
        Ok(())
    }

    /// Wait for the completion of the rebuilding threads
    fn wait_for_completion(
        &self,
        exit_receiver: Receiver<Result<(), std::io::Error>>,
    ) -> Result<(), std::io::Error> {
        let num_slots = self.snapshot_storage_lengths.len();
        let mut last_log_time = Instant::now();
        loop {
            select! {
                recv(exit_receiver) -> maybe_exit_signal => {
                    match maybe_exit_signal {
                        Ok(Ok(_)) => continue, // thread exited successfully
                        Ok(Err(err)) => { // thread exited with error
                            return Err(err);
                        }
                        Err(_) => break, // all threads have exited - channel disconnected
                    }
                }
                default(std::time::Duration::from_millis(100)) => {
                    let now = Instant::now();
                    if now.duration_since(last_log_time).as_millis() >= 2000 {
                        let num_processed_slots = self.processed_slot_count.load(Ordering::Relaxed);
                        let num_collisions = self.num_collisions.load(Ordering::Relaxed);
                        info!("rebuilt storages for {num_processed_slots}/{num_slots} slots with {num_collisions} collisions");
                        last_log_time = now;
                    }
                }
            }
        }

        Ok(())
    }

    /// Builds thread pool to rebuild with
    fn build_thread_pool(&self) -> ThreadPool {
        ThreadPoolBuilder::default()
            .num_threads(self.num_threads)
            .build()
            .unwrap()
    }
}

/// Used to determine if a filename is structured like a version file, bank file, or storage file
#[derive(PartialEq, Debug)]
enum SnapshotFileKind {
    Version,
    BankFields,
    Storage,
}

/// Determines `SnapshotFileKind` for `filename` if any
fn get_snapshot_file_kind(filename: &str) -> Option<SnapshotFileKind> {
    if VERSION_FILE_REGEX.is_match(filename) {
        Some(SnapshotFileKind::Version)
    } else if BANK_FIELDS_FILE_REGEX.is_match(filename) {
        Some(SnapshotFileKind::BankFields)
    } else if STORAGE_FILE_REGEX.is_match(filename) {
        Some(SnapshotFileKind::Storage)
    } else {
        None
    }
}

/// Get the slot and append vec id from the filename
pub(crate) fn get_slot_and_append_vec_id(filename: &str) -> (Slot, usize) {
    STORAGE_FILE_REGEX
        .captures(filename)
        .map(|cap| {
            let slot_str = cap.name("slot").map(|m| m.as_str()).unwrap();
            let id_str = cap.name("id").map(|m| m.as_str()).unwrap();
            let slot = slot_str.parse().unwrap();
            let id = id_str.parse().unwrap();
            (slot, id)
        })
        .unwrap()
}

#[cfg(test)]
mod tests {
    use {super::*, crate::append_vec::AppendVec};

    #[test]
    fn test_get_snapshot_file_kind() {
        assert_eq!(None, get_snapshot_file_kind("file.txt"));
        assert_eq!(
            Some(SnapshotFileKind::Version),
            get_snapshot_file_kind("version")
        );
        assert_eq!(
            Some(SnapshotFileKind::BankFields),
            get_snapshot_file_kind("1234")
        );
        assert_eq!(
            Some(SnapshotFileKind::Storage),
            get_snapshot_file_kind("1000.999")
        );
    }

    #[test]
    fn test_get_slot_and_append_vec_id() {
        let expected_slot = 12345;
        let expected_id = 9987;
        let (slot, id) =
            get_slot_and_append_vec_id(&AppendVec::file_name(expected_slot, expected_id));
        assert_eq!(expected_slot, slot);
        assert_eq!(expected_id, id);
    }
}
