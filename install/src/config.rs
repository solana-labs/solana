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
        let config = serde_yaml::from_reader(file)
            .map_err(|err| io::Error::new(io::ErrorKind::Other, format!("{err:?}")))?;
        Ok(config)
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
}
