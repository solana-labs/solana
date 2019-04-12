use crate::config::Config;
use crate::update_manifest::{SignedUpdateManifest, UpdateManifest};
use chrono::{Local, TimeZone};
use console::{style, Emoji};
use indicatif::{ProgressBar, ProgressStyle};
use ring::digest::{Context, Digest, SHA256};
use solana_client::rpc_client::RpcClient;
use solana_config_api::config_instruction;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{read_keypair, Keypair, KeypairUtil, Signable};
use solana_sdk::transaction::Transaction;
use std::fs::{self, File};
use std::io::{self, BufReader, Read};
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::SystemTime;
use std::time::{Duration, Instant};
use tempdir::TempDir;
use url::Url;

static TRUCK: Emoji = Emoji("🚚 ", "");
static LOOKING_GLASS: Emoji = Emoji("🔍 ", "");
static BULLET: Emoji = Emoji("• ", "* ");
static SPARKLE: Emoji = Emoji("✨ ", "");
static PACKAGE: Emoji = Emoji("📦 ", "");

/// Creates a new process bar for processing that will take an unknown amount of time
fn new_spinner_progress_bar() -> ProgressBar {
    let progress_bar = ProgressBar::new(42);
    progress_bar
        .set_style(ProgressStyle::default_spinner().template("{spinner:.green} {wide_msg}"));
    progress_bar.enable_steady_tick(100);
    progress_bar
}

/// Pretty print a "name value"
fn println_name_value(name: &str, value: &str) {
    println!("{} {}", style(name).bold(), value);
}

/// Downloads the release archive at `url` to a temporary location.  If `expected_sha256` is
/// Some(_), produce an error if the release SHA256 doesn't match.
///
/// Returns a tuple consisting of:
/// * TempDir - drop this value to clean up the temporary location
/// * PathBuf - path to the downloaded release (within `TempDir`)
/// * String  - SHA256 of the release
///
fn download_to_temp_archive(
    url: &str,
    expected_sha256: Option<&str>,
) -> Result<(TempDir, PathBuf, String), Box<dyn std::error::Error>> {
    fn sha256_digest<R: Read>(mut reader: R) -> Result<Digest, Box<dyn std::error::Error>> {
        let mut context = Context::new(&SHA256);
        let mut buffer = [0; 1024];

        loop {
            let count = reader.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            context.update(&buffer[..count]);
        }

        Ok(context.finish())
    }

    fn sha256_file_digest<P: AsRef<Path>>(path: P) -> Result<Digest, Box<dyn std::error::Error>> {
        let input = File::open(path)?;
        let reader = BufReader::new(input);
        sha256_digest(reader)
    }

    let url = Url::parse(url).map_err(|err| format!("Unable to parse {}: {}", url, err))?;

    let temp_dir = TempDir::new(clap::crate_name!())?;
    let temp_file = temp_dir.path().join("release.tar.bz2");

    let client = reqwest::Client::new();

    let progress_bar = new_spinner_progress_bar();
    progress_bar.set_message(&format!("{}Downloading...", TRUCK));

    let response = client.get(url).send()?;
    let download_size = {
        response
            .headers()
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|content_length| content_length.to_str().ok())
            .and_then(|content_length| content_length.parse().ok())
            .unwrap_or(0)
    };

    progress_bar.set_length(download_size);
    progress_bar.set_style(
        ProgressStyle::default_bar()
            .template(&format!(
                "{}{}{}",
                "{spinner:.green} ",
                TRUCK,
                "Downloading [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})"
            ))
            .progress_chars("=> "),
    );

    struct DownloadProgress<R> {
        progress_bar: ProgressBar,
        response: R,
    }

    impl<R: Read> Read for DownloadProgress<R> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.response.read(buf).map(|n| {
                self.progress_bar.inc(n as u64);
                n
            })
        }
    }

    let mut source = DownloadProgress {
        progress_bar,
        response,
    };

    let mut file = File::create(&temp_file)?;
    std::io::copy(&mut source, &mut file)?;

    let temp_file_sha256 = sha256_file_digest(&temp_file)
        .map_err(|err| format!("Unable to hash {:?}: {}", temp_file, err))?;
    let temp_file_sha256 = bs58::encode(temp_file_sha256).into_string();

    if expected_sha256.is_some() && expected_sha256 != Some(&temp_file_sha256) {
        Err(io::Error::new(io::ErrorKind::Other, "Incorrect hash"))?;
    }
    Ok((temp_dir, temp_file, temp_file_sha256))
}

/// Extracts the release archive into the specified directory
fn extract_release_archive(
    archive: &Path,
    extract_dir: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    use bzip2::bufread::BzDecoder;
    use tar::Archive;

    let progress_bar = new_spinner_progress_bar();
    progress_bar.set_message(&format!("{}Extracting...", PACKAGE));

    let _ = fs::remove_dir_all(extract_dir);
    fs::create_dir_all(extract_dir)?;

    let tar_bz2 = File::open(archive)?;
    let tar = BzDecoder::new(BufReader::new(tar_bz2));
    let mut release = Archive::new(tar);
    release.unpack(extract_dir)?;

    progress_bar.finish_and_clear();
    Ok(())
}

/// Reads the supported TARGET triple for the given release
fn load_release_target(release_dir: &Path) -> Result<String, Box<dyn std::error::Error>> {
    use serde_derive::Deserialize;
    #[derive(Deserialize, Debug)]
    pub struct ReleaseVersion {
        pub target: String,
        pub commit: String,
        channel: String,
    }

    let mut version_yml = PathBuf::from(release_dir);
    version_yml.push("solana-release");
    version_yml.push("version.yml");

    let file = File::open(&version_yml)?;
    let version: ReleaseVersion = serde_yaml::from_reader(file)?;
    Ok(version.target)
}

/// Time in seconds since the UNIX_EPOCH
fn timestamp_secs() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// Create an empty update manifest for the given `update_manifest_keypair` if it doesn't already
/// exist on the cluster
fn new_update_manifest(
    rpc_client: &RpcClient,
    from_keypair: &Keypair,
    update_manifest_keypair: &Keypair,
) -> Result<(), Box<dyn std::error::Error>> {
    if rpc_client
        .get_account_data(&update_manifest_keypair.pubkey())
        .is_err()
    {
        let recect_blockhash = rpc_client.get_recent_blockhash()?;

        let new_account = config_instruction::create_account::<SignedUpdateManifest>(
            &from_keypair.pubkey(),
            &update_manifest_keypair.pubkey(),
            1, // lamports
        );
        let mut transaction = Transaction::new_unsigned_instructions(vec![new_account]);
        transaction.sign(&[from_keypair], recect_blockhash);

        rpc_client.send_and_confirm_transaction(&mut transaction, from_keypair)?;
    }
    Ok(())
}

/// Update the update manifest on the cluster with new content
fn store_update_manifest(
    rpc_client: &RpcClient,
    from_keypair: &Keypair,
    update_manifest_keypair: &Keypair,
    update_manifest: &SignedUpdateManifest,
) -> Result<(), Box<dyn std::error::Error>> {
    let recect_blockhash = rpc_client.get_recent_blockhash()?;

    let new_store = config_instruction::store::<SignedUpdateManifest>(
        &from_keypair.pubkey(),
        &update_manifest_keypair.pubkey(),
        update_manifest,
    );
    let mut transaction = Transaction::new_unsigned_instructions(vec![new_store]);
    transaction.sign(&[from_keypair, update_manifest_keypair], recect_blockhash);
    rpc_client.send_and_confirm_transaction(&mut transaction, from_keypair)?;
    Ok(())
}

/// Read the current contents of the update manifest from the cluster
fn get_update_manifest(
    rpc_client: &RpcClient,
    update_manifest_pubkey: &Pubkey,
) -> Result<UpdateManifest, String> {
    let data = rpc_client
        .get_account_data(update_manifest_pubkey)
        .map_err(|err| format!("Unable to fetch update manifest: {}", err))?;

    let signed_update_manifest =
        SignedUpdateManifest::deserialize(update_manifest_pubkey, &data)
            .map_err(|err| format!("Unable to deserialize update manifest: {}", err))?;
    Ok(signed_update_manifest.manifest)
}

/// Bug the user if active_release_bin_dir is not in their PATH
fn check_env_path_for_bin_dir(config: &Config) {
    use std::env;

    let bin_dir = config.active_release_bin_dir();
    let found = match env::var_os("PATH") {
        Some(paths) => env::split_paths(&paths).any(|path| {
            if let Ok(path) = path.canonicalize() {
                if path == bin_dir {
                    return true;
                }
            }
            false
        }),
        None => false,
    };

    if !found {
        println!(
            "\nPlease update your PATH environment variable to include the solana programs:\n    PATH=\"{}:$PATH\"\n",
            bin_dir.to_str().unwrap()
        );
    }
}

pub fn init(
    config_file: &str,
    data_dir: &str,
    json_rpc_url: &str,
    update_manifest_pubkey: &Pubkey,
    no_modify_path: bool,
) -> Result<(), String> {
    let config = Config::new(data_dir, json_rpc_url, update_manifest_pubkey);
    config.save(config_file)?;
    update(config_file)?;

    let mut modified_rcfiles = false;
    let shell_export_string = format!(
        r#"export PATH="{}:$PATH""#,
        config.active_release_bin_dir().to_str().unwrap()
    );

    if !no_modify_path {
        // Look for sh, bash, and zsh rc files
        let mut rcfiles = vec![dirs::home_dir().map(|p| p.join(".profile"))];
        if let Ok(shell) = std::env::var("SHELL") {
            if shell.contains("zsh") {
                let zdotdir = std::env::var("ZDOTDIR")
                    .ok()
                    .map(PathBuf::from)
                    .or_else(dirs::home_dir);
                let zprofile = zdotdir.map(|p| p.join(".zprofile"));
                rcfiles.push(zprofile);
            }
        }

        if let Some(bash_profile) = dirs::home_dir().map(|p| p.join(".bash_profile")) {
            // Only update .bash_profile if it exists because creating .bash_profile
            // will cause .profile to not be read
            if bash_profile.exists() {
                rcfiles.push(Some(bash_profile));
            }
        }
        let rcfiles = rcfiles.into_iter().filter_map(|f| f.filter(|f| f.exists()));

        // For each rc file, append a PATH entry if not already present
        for rcfile in rcfiles {
            if !rcfile.exists() {
                continue;
            }

            fn read_file(path: &Path) -> io::Result<String> {
                let mut file = fs::OpenOptions::new().read(true).open(path)?;
                let mut contents = String::new();
                io::Read::read_to_string(&mut file, &mut contents)?;
                Ok(contents)
            }

            match read_file(&rcfile) {
                Err(err) => {
                    println!("Unable to read {:?}: {}", rcfile, err);
                }
                Ok(contents) => {
                    if !contents.contains(&shell_export_string) {
                        println!(
                            "Adding {} to {}",
                            style(&shell_export_string).italic(),
                            style(rcfile.to_str().unwrap()).bold()
                        );

                        fn append_file(dest: &Path, line: &str) -> io::Result<()> {
                            use std::io::Write;
                            let mut dest_file = fs::OpenOptions::new()
                                .write(true)
                                .append(true)
                                .create(true)
                                .open(dest)?;

                            writeln!(&mut dest_file, "{}", line)?;

                            dest_file.sync_data()?;

                            Ok(())
                        }
                        append_file(&rcfile, &shell_export_string).unwrap_or_else(|err| {
                            format!("Unable to append to {:?}: {}", rcfile, err);
                        });
                        modified_rcfiles = true;
                    }
                }
            }
        }
    }

    if modified_rcfiles {
        println!(
            "\n{}\n  {}\n",
            style("Close and reopen your terminal to apply the PATH changes or run the following in your existing shell:").bold().blue(),
            shell_export_string
       );
    } else {
        check_env_path_for_bin_dir(&config);
    }

    Ok(())
}

pub fn info(config_file: &str, local_info_only: bool) -> Result<Option<UpdateManifest>, String> {
    let config = Config::load(config_file)?;
    println_name_value("JSON RPC URL:", &config.json_rpc_url);
    println_name_value(
        "Update manifest pubkey:",
        &config.update_manifest_pubkey.to_string(),
    );

    fn print_update_manifest(update_manifest: &UpdateManifest) {
        let when = Local.timestamp(update_manifest.timestamp_secs as i64, 0);
        println_name_value(
            &format!("{}release date", BULLET),
            &when.format("%c").to_string(),
        );
        println_name_value(
            &format!("{}download URL", BULLET),
            &update_manifest.download_url,
        );
    }

    match config.current_update_manifest {
        Some(ref update_manifest) => {
            println_name_value("Installed version:", "");
            print_update_manifest(&update_manifest);
        }
        None => {
            println_name_value("Installed version:", "None");
        }
    }

    if local_info_only {
        Ok(None)
    } else {
        let progress_bar = new_spinner_progress_bar();
        progress_bar.set_message(&format!("{}Checking for updates...", LOOKING_GLASS));
        let rpc_client = RpcClient::new(config.json_rpc_url.clone());
        let manifest = get_update_manifest(&rpc_client, &config.update_manifest_pubkey)?;
        progress_bar.finish_and_clear();

        if Some(&manifest) == config.current_update_manifest.as_ref() {
            println!("\n{}", style("Installation is up to date").italic());
            Ok(None)
        } else {
            println!("\n{}", style("An update is available:").bold());
            print_update_manifest(&manifest);
            Ok(Some(manifest))
        }
    }
}

pub fn deploy(
    json_rpc_url: &str,
    from_keypair_file: &str,
    download_url: &str,
    update_manifest_keypair_file: &str,
) -> Result<(), String> {
    let from_keypair = read_keypair(from_keypair_file)
        .map_err(|err| format!("Unable to read {}: {}", from_keypair_file, err))?;
    let update_manifest_keypair = read_keypair(update_manifest_keypair_file)
        .map_err(|err| format!("Unable to read {}: {}", update_manifest_keypair_file, err))?;

    // Confirm the `json_rpc_url` is good and that `from_keypair` is a valid account
    let rpc_client = RpcClient::new(json_rpc_url.to_string());
    let progress_bar = new_spinner_progress_bar();
    progress_bar.set_message(&format!("{}Checking cluster...", LOOKING_GLASS));
    let balance = rpc_client
        .retry_get_balance(&from_keypair.pubkey(), 5)
        .map_err(|err| {
            format!(
                "Unable to get the account balance of {}: {}",
                from_keypair_file, err
            )
        })?;
    progress_bar.finish_and_clear();
    if balance.unwrap_or(0) == 0 {
        Err(format!("{} account balance is empty", from_keypair_file))?;
    }

    // Download the release
    let (temp_dir, temp_archive, temp_archive_sha256) =
        download_to_temp_archive(download_url, None)
            .map_err(|err| format!("Unable to download {}: {}", download_url, err))?;

    // Extract it and load the release version metadata
    let temp_release_dir = temp_dir.path().join("archive");
    extract_release_archive(&temp_archive, &temp_release_dir).map_err(|err| {
        format!(
            "Unable to extract {:?} into {:?}: {}",
            temp_archive, temp_release_dir, err
        )
    })?;

    let release_target = load_release_target(&temp_release_dir).map_err(|err| {
        format!(
            "Unable to load release target from {:?}: {}",
            temp_release_dir, err
        )
    })?;

    println_name_value("JSON RPC URL:", json_rpc_url);
    println_name_value("Update target:", &release_target);
    println_name_value(
        "Update manifest pubkey:",
        &update_manifest_keypair.pubkey().to_string(),
    );

    let progress_bar = new_spinner_progress_bar();
    progress_bar.set_message(&format!("{}Deploying update...", PACKAGE));

    // Construct an update manifest for the release
    let mut update_manifest = SignedUpdateManifest {
        account_pubkey: update_manifest_keypair.pubkey(),
        ..SignedUpdateManifest::default()
    };

    update_manifest.manifest.timestamp_secs = timestamp_secs();
    update_manifest.manifest.download_url = download_url.to_string();
    update_manifest.manifest.download_sha256 = temp_archive_sha256;

    update_manifest.sign(&update_manifest_keypair);
    assert!(update_manifest.verify());

    // Store the new update manifest on the cluster
    new_update_manifest(&rpc_client, &from_keypair, &update_manifest_keypair)
        .map_err(|err| format!("Unable to create update manifest: {}", err))?;
    store_update_manifest(
        &rpc_client,
        &from_keypair,
        &update_manifest_keypair,
        &update_manifest,
    )
    .map_err(|err| format!("Unable to store update manifest: {:?}", err))?;

    progress_bar.finish_and_clear();
    println!("  {}{}", SPARKLE, style("Deployment successful").bold());
    Ok(())
}

pub fn update(config_file: &str) -> Result<bool, String> {
    let update_manifest = info(config_file, false)?;
    if update_manifest.is_none() {
        return Ok(false);
    }
    let update_manifest = update_manifest.unwrap();

    if timestamp_secs()
        < u64::from_str_radix(crate::build_env::BUILD_SECONDS_SINCE_UNIX_EPOCH, 10).unwrap()
    {
        Err("Unable to update as system time seems unreliable".to_string())?
    }

    let mut config = Config::load(config_file)?;
    if let Some(ref current_update_manifest) = config.current_update_manifest {
        if update_manifest.timestamp_secs < current_update_manifest.timestamp_secs {
            Err("Unable to update to an older version".to_string())?
        }
    }

    let (_temp_dir, temp_archive, _temp_archive_sha256) = download_to_temp_archive(
        &update_manifest.download_url,
        Some(&update_manifest.download_sha256),
    )
    .map_err(|err| {
        format!(
            "Unable to download {}: {}",
            update_manifest.download_url, err
        )
    })?;

    let release_dir = config.release_dir(&update_manifest.download_sha256);

    extract_release_archive(&temp_archive, &release_dir).map_err(|err| {
        format!(
            "Unable to extract {:?} to {:?}: {}",
            temp_archive, release_dir, err
        )
    })?;

    let release_target = load_release_target(&release_dir).map_err(|err| {
        format!(
            "Unable to load release target from {:?}: {}",
            release_dir, err
        )
    })?;

    if release_target != crate::build_env::TARGET {
        Err(format!("Incompatible update target: {}", release_target))?;
    }

    let _ = fs::remove_dir_all(config.active_release_dir());
    std::os::unix::fs::symlink(
        release_dir.join("solana-release"),
        config.active_release_dir(),
    )
    .map_err(|err| {
        format!(
            "Unable to symlink {:?} to {:?}: {}",
            release_dir,
            config.active_release_dir(),
            err
        )
    })?;

    config.current_update_manifest = Some(update_manifest);
    config.save(config_file)?;

    println!("  {}{}", SPARKLE, style("Update successful").bold());
    Ok(true)
}

pub fn run(
    config_file: &str,
    program_name: &str,
    program_arguments: Vec<&str>,
) -> Result<(), String> {
    let config = Config::load(config_file)?;

    let full_program_path = config.active_release_bin_dir().join(program_name);
    if !full_program_path.exists() {
        Err(format!(
            "{} does not exist",
            full_program_path.to_str().unwrap()
        ))?;
    }

    let mut child_option: Option<std::process::Child> = None;
    let mut now = Instant::now();
    loop {
        child_option = match child_option {
            Some(mut child) => match child.try_wait() {
                Ok(Some(status)) => {
                    println_name_value(
                        &format!("{} exited with:", program_name),
                        &status.to_string(),
                    );
                    None
                }
                Ok(None) => Some(child),
                Err(err) => {
                    eprintln!("Error attempting to wait for program to exit: {}", err);
                    None
                }
            },
            None => {
                match std::process::Command::new(&full_program_path)
                    .args(&program_arguments)
                    .spawn()
                {
                    Ok(child) => Some(child),
                    Err(err) => {
                        eprintln!("Failed to spawn {}: {:?}", program_name, err);
                        None
                    }
                }
            }
        };

        if now.elapsed().as_secs() > config.update_poll_secs {
            match update(config_file) {
                Ok(true) => {
                    // Update successful, kill current process so it will be restart
                    if let Some(ref mut child) = child_option {
                        println!("Killing program: {:?}", child.kill());
                    }
                }
                Ok(false) => {} // No update available
                Err(err) => {
                    eprintln!("Failed to apply update: {:?}", err);
                }
            };
            now = Instant::now();
        }
        sleep(Duration::from_secs(1));
    }
}
