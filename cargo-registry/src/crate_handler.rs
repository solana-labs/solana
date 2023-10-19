use {
    crate::{
        client::{Client, RPCCommandConfig},
        sparse_index::{IndexEntry, RegistryIndex},
    },
    flate2::read::GzDecoder,
    hyper::body::Bytes,
    log::*,
    serde::{Deserialize, Serialize},
    serde_json::from_slice,
    sha2::{Digest, Sha256},
    solana_cli::program_v4::{process_deploy_program, read_and_verify_elf},
    solana_sdk::{
        signature::{Keypair, Signer},
        signer::EncodableKey,
    },
    std::{
        collections::BTreeMap,
        fs,
        mem::size_of,
        ops::Deref,
        path::{Path, PathBuf},
        sync::Arc,
    },
    tar::Archive,
    tempfile::{tempdir, TempDir},
};

pub(crate) type Error = Box<dyn std::error::Error + Send + Sync + 'static>;

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum DependencyType {
    Dev,
    Build,
    Normal,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub(crate) struct Dependency {
    pub name: String,
    pub version_req: String,
    pub features: Vec<String>,
    pub optional: bool,
    pub default_features: bool,
    pub target: Option<String>,
    pub kind: DependencyType,
    pub registry: Option<String>,
    pub explicit_name_in_toml: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(unused)]
pub(crate) struct PackageMetaData {
    pub name: String,
    pub vers: String,
    pub deps: Vec<Dependency>,
    pub features: BTreeMap<String, Vec<String>>,
    pub authors: Vec<String>,
    pub description: Option<String>,
    pub documentation: Option<String>,
    pub homepage: Option<String>,
    pub readme: Option<String>,
    pub readme_file: Option<String>,
    pub keywords: Vec<String>,
    pub categories: Vec<String>,
    pub license: Option<String>,
    pub license_file: Option<String>,
    pub repository: Option<String>,
    pub badges: BTreeMap<String, BTreeMap<String, String>>,
    pub links: Option<String>,
    pub rust_version: Option<String>,
}

impl PackageMetaData {
    fn new(bytes: &Bytes) -> serde_json::Result<(PackageMetaData, usize)> {
        let (json_length, sizeof_length) = Self::read_u32_length(bytes)?;
        let end_of_meta_data = sizeof_length.saturating_add(json_length as usize);
        let json_body = bytes.slice(sizeof_length..end_of_meta_data);
        from_slice::<PackageMetaData>(json_body.deref()).map(|data| (data, end_of_meta_data))
    }

    fn read_u32_length(bytes: &Bytes) -> serde_json::Result<(u32, usize)> {
        let sizeof_length = size_of::<u32>();
        let length_le = bytes.slice(0..sizeof_length);
        let length =
            u32::from_le_bytes(length_le.deref().try_into().expect("Failed to read length"));
        Ok((length, sizeof_length))
    }
}

pub(crate) struct Publisher {}

impl Publisher {
    fn make_path<P: AsRef<Path>>(tempdir: &TempDir, meta: &PackageMetaData, append: P) -> PathBuf {
        let mut path = tempdir.path().to_path_buf();
        path.push(format!("{}-{}/", meta.name, meta.vers));
        path.push(append);
        path
    }

    fn program_library_name(tempdir: &TempDir, meta: &PackageMetaData) -> Result<String, Error> {
        let toml_content = fs::read_to_string(Self::make_path(tempdir, meta, "Cargo.toml.orig"))?;
        let toml = toml_content.parse::<toml::Table>()?;
        let library_name = toml
            .get("lib")
            .and_then(|v| v.get("name"))
            .and_then(|v| v.as_str())
            .ok_or("Failed to get module name")?;
        Ok(library_name.to_string())
    }

    pub(crate) fn publish_crate(
        bytes: Bytes,
        client: Arc<Client>,
        index: Arc<RegistryIndex>,
    ) -> Result<(), Error> {
        let (meta_data, offset) = PackageMetaData::new(&bytes)?;

        let (_crate_file_length, length_size) =
            PackageMetaData::read_u32_length(&bytes.slice(offset..))?;
        let crate_bytes = bytes.slice(offset.saturating_add(length_size)..);
        let crate_cksum = format!("{:x}", Sha256::digest(&crate_bytes));

        let decoder = GzDecoder::new(crate_bytes.as_ref());
        let mut archive = Archive::new(decoder);

        let tempdir = tempdir()?;
        archive.unpack(tempdir.path())?;

        let command_config = RPCCommandConfig::new(client.as_ref());

        let lib_name = Self::program_library_name(&tempdir, &meta_data)?;

        let program_path = Self::make_path(&tempdir, &meta_data, format!("out/{}.so", lib_name))
            .into_os_string()
            .into_string()
            .map_err(|_| "Failed to get program file path")?;

        let program_data = read_and_verify_elf(program_path.as_ref())
            .map_err(|e| format!("failed to read the program: {}", e))?;

        let program_keypair = Keypair::read_from_file(Self::make_path(
            &tempdir,
            &meta_data,
            format!("out/{}-keypair.json", lib_name),
        ))
        .map_err(|e| format!("Failed to get keypair from the file: {}", e))?;

        info!("Deploying program at {:?}", program_keypair.pubkey());

        process_deploy_program(
            client.rpc_client.clone(),
            &command_config.0,
            &program_data,
            program_data.len() as u32,
            &program_keypair.pubkey(),
            Some(&program_keypair),
        )
        .map_err(|e| {
            error!("Failed to deploy the program: {}", e);
            format!("Failed to deploy the program: {}", e)
        })?;

        let mut entry: IndexEntry = meta_data.into();
        entry.cksum = crate_cksum;
        index.insert_entry(entry)?;

        info!("Successfully deployed the program");
        Ok(())
    }
}
