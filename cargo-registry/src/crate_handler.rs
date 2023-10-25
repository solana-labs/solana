use {
    crate::{
        client::{Client, RPCCommandConfig},
        sparse_index::{IndexEntry, RegistryIndex},
    },
    flate2::{
        read::{GzDecoder, GzEncoder},
        Compression,
    },
    hyper::body::Bytes,
    log::*,
    serde::{Deserialize, Serialize},
    serde_json::from_slice,
    sha2::{Digest, Sha256},
    solana_cli::program_v4::{process_deploy_program, process_dump, read_and_verify_elf},
    solana_sdk::{
        pubkey::Pubkey,
        signature::{Keypair, Signer},
        signer::EncodableKey,
    },
    std::{
        collections::BTreeMap,
        fs,
        io::{Cursor, Read},
        mem::size_of,
        ops::Deref,
        path::{Path, PathBuf},
        str::FromStr,
        sync::Arc,
    },
    tar::{Archive, Builder},
    tempfile::{tempdir, TempDir},
};

pub(crate) type Error = Box<dyn std::error::Error + Send + Sync + 'static>;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum DependencyType {
    Dev,
    Build,
    Normal,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Deserialize, Serialize)]
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

#[derive(Clone, Debug, Deserialize, Serialize)]
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

pub(crate) struct Program {
    path: String,
    id: Pubkey,
    _tempdir: Arc<TempDir>,
}

impl Program {
    fn deploy(&self, client: Arc<Client>, signer: &dyn Signer) -> Result<(), Error> {
        if self.id != signer.pubkey() {
            return Err("Signer doesn't match program ID".into());
        }

        let program_data = read_and_verify_elf(self.path.as_ref())
            .map_err(|e| format!("failed to read the program: {}", e))?;

        let command_config = RPCCommandConfig::new(client.as_ref());

        process_deploy_program(
            client.rpc_client.clone(),
            &command_config.0,
            &program_data,
            program_data.len() as u32,
            &signer.pubkey(),
            Some(signer),
        )
        .map_err(|e| {
            error!("Failed to deploy the program: {}", e);
            format!("Failed to deploy the program: {}", e)
        })?;

        Ok(())
    }

    fn dump(&self, client: Arc<Client>) -> Result<(), Error> {
        info!("Fetching program {:?}", self.id);
        let command_config = RPCCommandConfig::new(client.as_ref());

        process_dump(
            client.rpc_client.clone(),
            command_config.0.commitment,
            Some(self.id),
            &self.path,
        )
        .map_err(|e| {
            error!("Failed to fetch the program: {}", e);
            format!("Failed to fetch the program: {}", e)
        })?;

        Ok(())
    }

    pub(crate) fn crate_name_to_program_id(crate_name: &str) -> Option<Pubkey> {
        crate_name
            .split_once('-')
            .and_then(|(_prefix, id_str)| Pubkey::from_str(id_str).ok())
    }
}

impl From<&UnpackedCrate> for Program {
    fn from(value: &UnpackedCrate) -> Self {
        Self {
            path: value.program_path.clone(),
            id: value.program_id,
            _tempdir: value.tempdir.clone(),
        }
    }
}

pub(crate) struct CratePackage(pub(crate) Bytes);

impl From<UnpackedCrate> for Result<CratePackage, Error> {
    fn from(value: UnpackedCrate) -> Self {
        let mut archive = Builder::new(Vec::new());
        archive.append_dir_all(".", value.tempdir.path())?;
        let data = archive.into_inner()?;
        let reader = Cursor::new(data);
        let mut encoder = GzEncoder::new(reader, Compression::fast());
        let mut zipped_data = Vec::new();
        encoder.read_to_end(&mut zipped_data)?;

        let meta_str = serde_json::to_string(&value.meta)?;

        let sizeof_length = size_of::<u32>();
        let mut packed = Vec::with_capacity(
            sizeof_length
                .saturating_add(meta_str.len())
                .saturating_add(sizeof_length)
                .saturating_add(zipped_data.len()),
        );

        packed[..sizeof_length].copy_from_slice(&u32::to_le_bytes(meta_str.len() as u32));
        let offset = sizeof_length;
        let end = offset.saturating_add(meta_str.len());
        packed[offset..end].copy_from_slice(meta_str.as_bytes());
        let offset = end;
        let end = offset.saturating_add(sizeof_length);
        packed[offset..end].copy_from_slice(&u32::to_le_bytes(zipped_data.len() as u32));
        let offset = end;
        packed[offset..].copy_from_slice(&zipped_data);

        Ok(CratePackage(Bytes::from(packed)))
    }
}

pub(crate) struct UnpackedCrate {
    meta: PackageMetaData,
    cksum: String,
    tempdir: Arc<TempDir>,
    program_path: String,
    program_id: Pubkey,
    keypair: Option<Keypair>,
}

impl From<CratePackage> for Result<UnpackedCrate, Error> {
    fn from(value: CratePackage) -> Self {
        let bytes = value.0;
        let (meta, offset) = PackageMetaData::new(&bytes)?;

        let (_crate_file_length, length_size) =
            PackageMetaData::read_u32_length(&bytes.slice(offset..))?;
        let crate_bytes = bytes.slice(offset.saturating_add(length_size)..);
        let cksum = format!("{:x}", Sha256::digest(&crate_bytes));

        let decoder = GzDecoder::new(crate_bytes.as_ref());
        let mut archive = Archive::new(decoder);

        let tempdir = tempdir()?;
        archive.unpack(tempdir.path())?;

        let lib_name = UnpackedCrate::program_library_name(&tempdir, &meta)?;

        let program_path =
            UnpackedCrate::make_path(&tempdir, &meta, format!("out/{}.so", lib_name))
                .into_os_string()
                .into_string()
                .map_err(|_| "Failed to get program file path")?;

        let keypair = Keypair::read_from_file(UnpackedCrate::make_path(
            &tempdir,
            &meta,
            format!("out/{}-keypair.json", lib_name),
        ))
        .map_err(|e| format!("Failed to get keypair from the file: {}", e))?;

        Ok(UnpackedCrate {
            meta,
            cksum,
            tempdir: Arc::new(tempdir),
            program_path,
            program_id: keypair.pubkey(),
            keypair: Some(keypair),
        })
    }
}

impl UnpackedCrate {
    pub(crate) fn publish(
        &self,
        client: Arc<Client>,
        index: Arc<RegistryIndex>,
    ) -> Result<(), Error> {
        let Some(signer) = &self.keypair else {
            return Err("No signer provided for the program deployment".into());
        };

        Program::from(self).deploy(client, signer)?;

        let mut entry: IndexEntry = self.meta.clone().into();
        entry.cksum = self.cksum.clone();
        index.insert_entry(entry)?;

        info!("Successfully deployed the program");
        Ok(())
    }

    pub(crate) fn fetch_index(id: Pubkey, client: Arc<Client>) -> Result<IndexEntry, Error> {
        let (_program, unpacked_crate) = Self::fetch_program(id, client)?;

        let mut entry: IndexEntry = unpacked_crate.meta.clone().into();
        entry.cksum = unpacked_crate.cksum.clone();

        Ok(entry)
    }

    #[allow(dead_code)]
    pub(crate) fn fetch(id: Pubkey, client: Arc<Client>) -> Result<CratePackage, Error> {
        let (_program, unpacked_crate) = Self::fetch_program(id, client)?;
        UnpackedCrate::into(unpacked_crate)
    }

    fn fetch_program(id: Pubkey, client: Arc<Client>) -> Result<(Program, UnpackedCrate), Error> {
        let crate_obj = Self::new_empty(id)?;
        let program = Program::from(&crate_obj);
        program.dump(client)?;

        // Decompile the program
        // Generate a Cargo.toml

        Ok((program, crate_obj))
    }

    fn new_empty(id: Pubkey) -> Result<Self, Error> {
        let meta = PackageMetaData {
            name: id.to_string(),
            vers: "0.1".to_string(),
            deps: vec![],
            features: BTreeMap::new(),
            authors: vec![],
            description: None,
            documentation: None,
            homepage: None,
            readme: None,
            readme_file: None,
            keywords: vec![],
            categories: vec![],
            license: None,
            license_file: None,
            repository: None,
            badges: BTreeMap::new(),
            links: None,
            rust_version: None,
        };

        let tempdir = tempdir()?;

        let program_path = Self::make_path(&tempdir, &meta, format!("out/{}.so", id))
            .into_os_string()
            .into_string()
            .map_err(|_| "Failed to get program file path")?;

        Ok(Self {
            meta,
            cksum: "".to_string(),
            tempdir: Arc::new(tempdir),
            program_path,
            program_id: id,
            keypair: None,
        })
    }

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
}
