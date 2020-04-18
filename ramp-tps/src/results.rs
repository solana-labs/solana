use log::*;
use serde::{Serialize, Serializer};
use std::{
    collections::{BTreeMap, HashMap},
    error::Error,
    fs::File,
    io::ErrorKind,
    process::exit,
    str::FromStr,
};

const ROUND_KEY_PREFIX: &str = "round-";

#[derive(Eq, PartialEq, Ord, PartialOrd)]
struct Round(u32);

impl Serialize for Round {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&format!("{}{}", ROUND_KEY_PREFIX, self.0))
    }
}

pub struct Results {
    file_path: String,
    results: BTreeMap<Round, Vec<String>>,
}

impl Results {
    /// Keep any result entries which occurred before the starting round.
    pub fn new(
        file_path: String,
        mut previous_results: HashMap<String, Vec<String>>,
        start_round: u32,
    ) -> Self {
        let mut results: BTreeMap<Round, Vec<String>> = BTreeMap::new();
        previous_results.drain().for_each(|(key, value)| {
            if key.starts_with(ROUND_KEY_PREFIX) {
                let round_str = &key[ROUND_KEY_PREFIX.len()..];
                dbg!(round_str);
                if let Ok(round) = u32::from_str(round_str) {
                    if round < start_round {
                        results.insert(Round(round), value);
                    }
                }
            }
        });

        Results { file_path, results }
    }

    // Reads the previous results file and if it exists, parses the contents
    pub fn read(file_path: &str) -> HashMap<String, Vec<String>> {
        match File::open(file_path) {
            Ok(file) => serde_yaml::from_reader(&file)
                .map_err(|err| {
                    warn!("Failed to recover previous results: {}", err);
                })
                .unwrap_or_default(),
            Err(err) => match err.kind() {
                ErrorKind::NotFound => {
                    // Check that we can write to this file
                    File::create(file_path).unwrap_or_else(|err| {
                        eprintln!(
                            "Error: Unable to create --results-file {}: {}",
                            file_path, err
                        );
                        exit(1);
                    });
                    HashMap::new()
                }
                err => {
                    eprintln!(
                        "Error: Unable to open --results-file {}: {:?}",
                        file_path, err
                    );
                    exit(1);
                }
            },
        }
    }

    /// Record the remaining validators after each TPS round
    pub fn record(&mut self, round: u32, validators: Vec<String>) -> Result<(), Box<dyn Error>> {
        self.results.insert(Round(round), validators);
        let file = File::create(&self.file_path)?;
        serde_yaml::to_writer(&file, &self.results)?;
        Ok(())
    }
}
