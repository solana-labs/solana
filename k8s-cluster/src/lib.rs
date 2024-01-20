#![allow(clippy::arithmetic_side_effects)]
use {
    bzip2::bufread::BzDecoder,
    console::Emoji,
    indexmap::IndexMap,
    indicatif::{ProgressBar, ProgressStyle},
    lazy_static::lazy_static,
    log::*,
    std::{
        env,
        fs::{self, File},
        io::{self, BufReader, Cursor, Read},
        num::NonZeroUsize,
        path::{Path, PathBuf},
        time::Duration,
    },
    tar::Archive,
    url::Url,
};

lazy_static! {
    #[derive(Debug)]
    static ref SOLANA_ROOT: PathBuf = get_solana_root();

    #[derive(Debug)]
    pub static ref LEDGER_DIR: PathBuf = SOLANA_ROOT.join("config-k8s/bootstrap-validator");
}

pub fn initialize_globals() {
    let _ = *SOLANA_ROOT; // Force initialization of lazy_static
}

pub mod docker;
pub mod genesis;
pub mod k8s_helpers;
pub mod kubernetes;
pub mod ledger_helper;
pub mod release;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ValidatorType {
    Bootstrap,
    Standard,
    NonVoting,
    Client,
}

impl std::fmt::Display for ValidatorType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            ValidatorType::Bootstrap => write!(f, "bootstrap"),
            ValidatorType::Standard => write!(f, "validator"),
            ValidatorType::NonVoting => write!(f, "non-voting"),
            ValidatorType::Client => write!(f, "client"),
        }
    }
}

pub fn get_solana_root() -> PathBuf {
    let solana_root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("$CARGO_MANIFEST_DIR"))
        .parent()
        .expect("Failed to get Solana root directory")
        .to_path_buf();
    solana_root
}

#[macro_export]
macro_rules! boxed_error {
    ($message:expr) => {
        Box::new(std::io::Error::new(std::io::ErrorKind::Other, $message)) as Box<dyn Error + Send>
    };
}

static TRUCK: Emoji = Emoji("🚚 ", "");
static PACKAGE: Emoji = Emoji("📦 ", "");
static ROCKET: Emoji = Emoji("🚀 ", "");
static WRITING: Emoji = Emoji("🖊️ ", "");
static SUN: Emoji = Emoji("🌞 ", "");
static BUILD: Emoji = Emoji("👷 ", "");

/// Creates a new process bar for processing that will take an unknown amount of time
pub fn new_spinner_progress_bar() -> ProgressBar {
    let progress_bar = ProgressBar::new(42);
    progress_bar.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {wide_msg}")
            .expect("ProgresStyle::template direct input to be correct"),
    );
    progress_bar.enable_steady_tick(Duration::from_millis(100));
    progress_bar
}

pub fn extract_release_archive(
    archive: &Path,
    extract_dir: &Path,
    file_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let progress_bar = new_spinner_progress_bar();
    progress_bar.set_message(format!("{PACKAGE}Extracting..."));

    if extract_dir.exists() {
        fs::remove_dir_all(extract_dir)?;
    }
    fs::create_dir_all(extract_dir)?;

    let tmp_extract_dir = extract_dir.with_file_name("tmp-extract");

    if tmp_extract_dir.exists() {
        let _ = fs::remove_dir_all(&tmp_extract_dir);
    }
    fs::create_dir_all(&tmp_extract_dir)?;

    let tar_bz2 = File::open(archive)?;
    let tar = BzDecoder::new(BufReader::new(tar_bz2));
    let mut release = Archive::new(tar);
    release.unpack(&tmp_extract_dir)?;

    for entry in tmp_extract_dir.join(file_name).read_dir()? {
        let entry = entry?;
        let entry_path = entry.path();
        let target_entry_path = extract_dir.join(entry_path.file_name().unwrap());
        fs::rename(entry_path, target_entry_path)?;
    }

    // Remove the tmp-extract directory
    fs::remove_dir_all(tmp_extract_dir)?;
    progress_bar.finish_and_clear();
    Ok(())
}

pub async fn download_to_temp(
    url: &str,
    file_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let progress_bar = new_spinner_progress_bar();
    progress_bar.set_message(format!("{TRUCK}Downloading..."));

    let url = Url::parse(url).map_err(|err| format!("Unable to parse {url}: {err}"))?;

    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .build()?;

    let response = client.get(url.as_str()).send().await?;
    // let file_name: PathBuf = SOLANA_ROOT.join("solana-release.tar.bz2");
    let file_name: PathBuf = SOLANA_ROOT.join(file_name);
    let mut out = File::create(file_name).expect("failed to create file");
    let mut content = Cursor::new(response.bytes().await?);
    std::io::copy(&mut content, &mut out)?;

    progress_bar.finish_and_clear();
    Ok(())
}

pub fn cat_file(path: &PathBuf) -> io::Result<()> {
    let mut file = fs::File::open(path)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;

    info!("{}", contents);

    Ok(())
}

pub fn parse_and_format_bench_tps_args(bench_tps_args: Option<&str>) -> Option<Vec<String>> {
    bench_tps_args.map(|args| {
        args.split_whitespace()
            .filter_map(|arg| arg.split_once('='))
            .flat_map(|(key, value)| vec![format!("--{}", key), value.to_string()])
            .collect()
    })
}

pub fn calculate_stake_allocations(
    total_sol: f64,
    num_validators: i32,
    distribution: &mut Vec<u8>,
) -> Result<(Vec<f64>, Vec<f64>), String> {
    if distribution.iter().sum::<u8>() != 100 {
        return Err("The sum of distribution percentages must be 100".to_string());
    }
    let num_validators_usize =
        usize::try_from(num_validators).map_err(|_| "num validators conversion failed.")?;
    if num_validators_usize < distribution.len() {
        return Err(format!(
            "num_validators < distribution.len(). {} < {}",
            num_validators,
            distribution.len()
        ));
    }
    let mut allocations: Vec<f64> = Vec::with_capacity(num_validators as usize);
    let mut stake_per_bucket: Vec<f64> = Vec::with_capacity(distribution.len());

    let mut buckets: IndexMap<u8, usize> = IndexMap::new();
    distribution.sort_by(|a, b| b.cmp(a)); // sort largest to smallest

    let nodes_per_stake_grouping = NonZeroUsize::new(num_validators_usize / distribution.len())
        .ok_or_else(|| format!("Fewer validators than distribution called for. num_validators: {}, distribution_len: {}", num_validators_usize, distribution.len()))?;

    let num_extra_validators = num_validators_usize % distribution.len();
    if num_extra_validators != 0 {
        warn!("WARNING: number of desired validators does not evenly split across desired distribution. \
        additional validators added one by one to distribution buckets starting with heaviest buckets by stake ({:?}%)
        num_validators: {}, distribution_len: {}, extra_validators: {}", distribution.first(), num_validators, distribution.len(), num_extra_validators);
    }

    for (i, percentage) in distribution.iter().enumerate() {
        let mut nodes_per_bucket = nodes_per_stake_grouping.get();
        if i < num_extra_validators {
            nodes_per_bucket = nodes_per_bucket.saturating_add(1);
        }
        buckets.insert(*percentage, nodes_per_bucket);
    }

    for (percent, num_nodes) in buckets.iter() {
        let total_sol_per_bucket = total_sol * (*percent as f64 / 100.0);
        stake_per_bucket.push(total_sol_per_bucket);

        let num_nodes_f64 = *num_nodes as f64;
        if num_nodes_f64 == 0.0 {
            return Err("num_nodes is 0. we can't divide by zero".to_string());
        }

        let allocation_per_node = total_sol_per_bucket / num_nodes_f64;
        for _ in 0..*num_nodes {
            allocations.push(allocation_per_node);
        }
    }
    Ok((stake_per_bucket, allocations))
}

#[test]
fn test_stake_allocations() {
    solana_logger::setup();
    let total_sol = 1000000.0;

    // No extra validators. Test num_validators % distribution.len() == 0
    let num_validators = 10;
    let mut distribution = vec![40, 30, 18, 10, 2];

    match calculate_stake_allocations(total_sol, num_validators, &mut distribution) {
        Ok((stake_per_bucket, allocations)) => {
            assert_eq!(
                vec![
                    200000.0, 200000.0, 150000.0, 150000.0, 90000.0, 90000.0, 50000.0, 50000.0,
                    10000.0, 10000.0
                ],
                allocations
            );
            assert_eq!(
                vec![400000.0, 300000.0, 180000.0, 100000.0, 20000.0],
                stake_per_bucket
            );
        }
        Err(err) => error!("calculate_stake_allocation() failed: {}", err),
    }

    // Three extra validators. Test num_validators % distribution.len() == 3
    let num_validators = 13;
    let mut distribution = vec![42, 33, 12, 10, 3];

    match calculate_stake_allocations(total_sol, num_validators, &mut distribution) {
        Ok((stake_per_bucket, allocations)) => {
            assert_eq!(
                vec![
                    140000.0, 140000.0, 140000.0, 110000.0, 110000.0, 110000.0, 40000.0, 40000.0,
                    40000.0, 50000.0, 50000.0, 15000.0, 15000.0
                ],
                allocations
            );
            assert_eq!(
                vec![420000.0, 330000.0, 120000.0, 100000.0, 30000.0],
                stake_per_bucket
            );
        }
        Err(err) => error!("calculate_stake_allocation() failed: {}", err),
    }

    // Too few validators. Test num_validators < distribution.len()
    let num_validators = 2;
    let mut distribution = vec![50, 40, 10];

    match calculate_stake_allocations(total_sol, num_validators, &mut distribution) {
        Ok((_, _)) =>
            panic!("ERROR. should not be here. calculate_stake_allocations() should return too few validators error"),
        Err(err) => assert_eq!("num_validators < distribution.len(). 2 < 3".to_string(), err),
    }

    // Sum of distribution should be 100
    let num_validators = 3;
    let mut distribution = vec![50, 40, 5];

    match calculate_stake_allocations(total_sol, num_validators, &mut distribution) {
        Ok((_, _)) => panic!(
            "ERROR. should not be here. calculate_stake_allocations() should sum to 100 error"
        ),
        Err(err) => assert_eq!(
            "The sum of distribution percentages must be 100".to_string(),
            err
        ),
    }
}

pub fn add_tag_to_name(name: &str, tag: &str) -> String {
    let mut name_with_tag = name.to_string();
    name_with_tag.push('-');
    name_with_tag.push_str(tag);
    name_with_tag
}
