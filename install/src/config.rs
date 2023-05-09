use {
    crate::update_manifest::UpdateManifest,
    serde::{Deserialize, Serialize},
    solana_sdk::pubkey::Pubkey,
    std::{
        fs::{create_dir_all, File},
        io::{self, Write},
        path::{Path, PathBuf},
    },
};

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
pub enum ExplicitRelease {
    Semver(String),
    Channel(String),
}

#[derive(Serialize, Deserialize, Default, Debug, PartialEq, Eq)]
pub struct Config {
    pub json_rpc_url: String,
    pub update_manifest_pubkey: Pubkey,
    pub current_update_manifest: Option<UpdateManifest>,
    pub update_poll_secs: u64,
    pub explicit_release: Option<ExplicitRelease>,
    pub releases_dir: PathBuf,
    active_release_dir: PathBuf,
}

const LEGACY_FMT_LOAD_ERR: &str =
    "explicit_release: invalid type: map, expected a YAML tag starting with '!'";

impl Config {
    pub fn new(
        data_dir: &str,
        json_rpc_url: &str,
        update_manifest_pubkey: &Pubkey,
        explicit_release: Option<ExplicitRelease>,
    ) -> Self {
        Self {
            json_rpc_url: json_rpc_url.to_string(),
            update_manifest_pubkey: *update_manifest_pubkey,
            current_update_manifest: None,
            update_poll_secs: 60 * 60, // check for updates once an hour
            explicit_release,
            releases_dir: PathBuf::from(data_dir).join("releases"),
            active_release_dir: PathBuf::from(data_dir).join("active_release"),
        }
    }

    fn _load(config_file: &str) -> Result<Self, io::Error> {
        let file = File::open(config_file)?;
        serde_yaml::from_reader(file).or_else(|err| {
            let err_string = format!("{err:?}");
            if err_string.contains(LEGACY_FMT_LOAD_ERR) {
                // looks like a config written by serde_yaml <0.9.0.
                // let's try to upgrade it
                Self::try_migrate_08(config_file)
                    .map_err(|_| io::Error::new(io::ErrorKind::Other, err_string))
            } else {
                Err(io::Error::new(io::ErrorKind::Other, err_string))
            }
        })
    }

    fn try_migrate_08(config_file: &str) -> Result<Self, io::Error> {
        eprintln!("attempting to upgrade legacy config file");
        let bak_filename = config_file.to_string() + ".bak";
        std::fs::copy(config_file, &bak_filename)?;
        let result = File::open(config_file).and_then(|file| {
            serde_yaml_08::from_reader(file)
                .map_err(|err| io::Error::new(io::ErrorKind::Other, format!("{err:?}")))
                .and_then(|config_08: Self| {
                    let save = config_08._save(config_file).map(|_| config_08);
                    if save.is_ok() {
                        let _ = std::fs::remove_file(&bak_filename);
                    }
                    save
                })
        });
        if result.is_err() {
            eprintln!("config upgrade failed! restoring orignal");
            let restored = std::fs::copy(&bak_filename, config_file)
                .and_then(|_| std::fs::remove_file(&bak_filename));
            if restored.is_err() {
                eprintln!("restoration failed! original: `{bak_filename}`");
            } else {
                eprintln!("restoration succeeded!");
            }
        } else {
            eprintln!("config upgrade succeeded!");
        }
        result
    }

    pub fn load(config_file: &str) -> Result<Self, String> {
        Self::_load(config_file).map_err(|err| format!("Unable to load {config_file}: {err:?}"))
    }

    fn _save(&self, config_file: &str) -> Result<(), io::Error> {
        let serialized = serde_yaml::to_string(self)
            .map_err(|err| io::Error::new(io::ErrorKind::Other, format!("{err:?}")))?;

        if let Some(outdir) = Path::new(&config_file).parent() {
            create_dir_all(outdir)?;
        }
        let mut file = File::create(config_file)?;
        file.write_all(b"---\n")?;
        file.write_all(&serialized.into_bytes())?;

        Ok(())
    }

    pub fn save(&self, config_file: &str) -> Result<(), String> {
        self._save(config_file)
            .map_err(|err| format!("Unable to save {config_file}: {err:?}"))
    }

    pub fn active_release_dir(&self) -> &PathBuf {
        &self.active_release_dir
    }

    pub fn active_release_bin_dir(&self) -> PathBuf {
        self.active_release_dir.join("bin")
    }

    pub fn release_dir(&self, release_id: &str) -> PathBuf {
        self.releases_dir.join(release_id)
    }
}

#[cfg(test)]
mod test {
    use {
        super::*,
        scopeguard::defer,
        std::{
            env,
            fs::{read_to_string, remove_file},
        },
    };

    #[test]
    fn test_save() {
        let root_dir = env::var("CARGO_MANIFEST_DIR").expect("$CARGO_MANIFEST_DIR");
        let json_rpc_url = "https://api.mainnet-beta.solana.com";
        let pubkey = Pubkey::default();
        let config_name = "config.yaml";
        let config_path = format!("{root_dir}/{config_name}");

        let config = Config::new(&root_dir, json_rpc_url, &pubkey, None);

        assert_eq!(config.save(config_name), Ok(()));
        defer! {
            remove_file(&config_path).unwrap();
        }

        assert_eq!(
            read_to_string(&config_path).unwrap(),
            format!(
                "---
json_rpc_url: https://api.mainnet-beta.solana.com
update_manifest_pubkey:
- 0
- 0
- 0
- 0
- 0
- 0
- 0
- 0
- 0
- 0
- 0
- 0
- 0
- 0
- 0
- 0
- 0
- 0
- 0
- 0
- 0
- 0
- 0
- 0
- 0
- 0
- 0
- 0
- 0
- 0
- 0
- 0
current_update_manifest: null
update_poll_secs: 3600
explicit_release: null
releases_dir: {root_dir}/releases
active_release_dir: {root_dir}/active_release
"
            ),
        );
    }

    #[test]
    fn test_load_serde_yaml_v_0_8_config() {
        let file_name = "config.yml";
        let mut file = File::create(file_name).unwrap();
        defer! {
            remove_file(file_name).unwrap();
        }

        let root_dir = "/home/sol/.local/share/solana/install";

        writeln!(
            file,
            "---
json_rpc_url: \"http://api.devnet.solana.com\"
update_manifest_pubkey:
  - 0
  - 0
  - 0
  - 0
  - 0
  - 0
  - 0
  - 0
  - 0
  - 0
  - 0
  - 0
  - 0
  - 0
  - 0
  - 0
  - 0
  - 0
  - 0
  - 0
  - 0
  - 0
  - 0
  - 0
  - 0
  - 0
  - 0
  - 0
  - 0
  - 0
  - 0
  - 0
current_update_manifest: ~
update_poll_secs: 3600
explicit_release:
  Semver: 1.13.6
releases_dir: {root_dir}/releases
active_release_dir: {root_dir}/active_release
"
        )
        .unwrap();
        let config = Config::load(file_name).unwrap();
        assert_eq!(
            config,
            Config {
                json_rpc_url: String::from("http://api.devnet.solana.com"),
                update_manifest_pubkey: Pubkey::default(),
                current_update_manifest: None,
                update_poll_secs: 3600,
                explicit_release: Some(ExplicitRelease::Semver(String::from("1.13.6"))),
                releases_dir: PathBuf::from(format!("{root_dir}/releases")),
                active_release_dir: PathBuf::from(format!("{root_dir}/active_release")),
            },
        );
    }
}
