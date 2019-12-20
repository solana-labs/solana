use serde_derive::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::Path;
#[cfg(unix)]
use std::thread;

pub fn datapoint(_name: &'static str) {
    #[cfg(unix)]
    {
        let allocated = jemalloc_ctl::thread::allocatedp::mib().unwrap();
        let allocated = allocated.read().unwrap();
        let mem = allocated.get();
        solana_metrics::datapoint_debug!("thread-memory", (_name, mem as i64, i64));
    }
}

pub struct Allocatedp {
    #[cfg(unix)]
    allocated: jemalloc_ctl::thread::ThreadLocal<u64>,
}

impl Allocatedp {
    pub fn default() -> Self {
        #[cfg(unix)]
        {
            let allocated = jemalloc_ctl::thread::allocatedp::mib().unwrap();
            let allocated = allocated.read().unwrap();
            Self { allocated }
        }
        #[cfg(not(unix))]
        Self {}
    }

    pub fn get(&self) -> u64 {
        #[cfg(unix)]
        {
            self.allocated.get()
        }
        #[cfg(not(unix))]
        0
    }
}

#[derive(PartialEq, Debug, Serialize, Deserialize)]
struct MapFile(HashMap<String, u64>);

impl MapFile {
    pub fn write_to_file(&self, path: &Path) {
        let mut file = fs::File::create(path).expect("Failed to create / write a file.");
        let str = serde_json::to_string_pretty(self).expect("Error serializing the file.");
        if let Err(err) = file.write_all(str.as_bytes()) {
            panic!("Failed to write a file {}", err);
        }
    }

    pub fn from_file(path: &Path) -> Self {
        let mut file = fs::File::open(path).expect("Could not open a file.");
        let mut content = String::new();
        file.read_to_string(&mut content)
            .expect("Could not read from key file.");
        serde_json::from_str(&content).expect("Failed to deserialize KeyFile")
    }

    pub fn default() {
        let path = Path::new("farf/file.txt");
        if path.exists() {
            fs::remove_file(&path).expect("Could not delete a file.");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_thread_memory_map() {
        let adp = Allocatedp::default();
        let mem = adp.get();

        let current_thread = thread::current();
        let name = current_thread.name().unwrap();

        let mut map: HashMap<String, u64> = HashMap::new();
        map.insert(name.to_string(), mem);

        let file = MapFile(map);

        let path = Path::new("farf/file.txt");
        file.write_to_file(&path);

        let new_file = MapFile::from_file(&path);

        assert_eq!(file, new_file);

        MapFile::default();
    }
}
