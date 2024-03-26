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
        sync::Arc,
    },
    tar::{Archive, Builder, HeaderMode},
    tempfile::{tempdir, TempDir},
};

const APPEND_CRATE_TO_ELF: bool = true;

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
    meta: PackageMetaData,
    packed_crate: PackedCrate,
}

impl Program {
    fn deploy(&self, client: Arc<Client>, signer: &dyn Signer) -> Result<(), Error> {
        if self.id != signer.pubkey() {
            return Err("Signer doesn't match program ID".into());
        }

        let mut program_data = read_and_verify_elf(self.path.as_ref())
            .map_err(|e| format!("failed to read the program: {}", e))?;

        if APPEND_CRATE_TO_ELF {
            let program_id_str = Program::program_id_to_crate_name(self.id);
            let crate_tar_gz =
                PackedCrate::new_rebased(&self.packed_crate, &self.meta, &program_id_str)?;
            let crate_len = u32::to_le_bytes(crate_tar_gz.0.len() as u32);
            program_data.extend_from_slice(&crate_tar_gz.0);
            program_data.extend_from_slice(&crate_len);
        }
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

    fn dump(&mut self, client: Arc<Client>) -> Result<(), Error> {
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

        if APPEND_CRATE_TO_ELF {
            let Ok(buffer) = fs::read(&self.path) else {
                return Err("Failed to read the program file".into());
            };

            let data = Bytes::from(buffer);

            let data_len = data.len();
            let sizeof_length = size_of::<u32>();

            // The crate length is at the tail of the data buffer, as 4 LE bytes.
            let length_le = data.slice(data_len.saturating_sub(sizeof_length)..data_len);
            let length =
                u32::from_le_bytes(length_le.deref().try_into().expect("Failed to read length"));

            let crate_start = data_len
                .saturating_sub(sizeof_length)
                .saturating_sub(length as usize);
            let crate_end = data_len.saturating_sub(sizeof_length);

            self.packed_crate = PackedCrate(Bytes::copy_from_slice(&data[crate_start..crate_end]));
        }
        Ok(())
    }

    pub(crate) fn crate_name_to_program_id(crate_name: &str) -> Option<Pubkey> {
        let (_, id_str) = crate_name.split_once('-')?;
        hex::decode(id_str)
            .ok()
            .and_then(|bytes| Pubkey::try_from(bytes).ok())
    }

    fn program_id_to_crate_name(id: Pubkey) -> String {
        format!("sol-{}", hex::encode(id.to_bytes()))
    }
}

impl From<&UnpackedCrate> for Program {
    fn from(value: &UnpackedCrate) -> Self {
        Self {
            path: value.program_path.clone(),
            id: value.program_id,
            _tempdir: value.tempdir.clone(),
            meta: value.meta.clone(),
            packed_crate: value.packed_crate.clone(),
        }
    }
}

/// Contents of a .crate file
#[derive(Clone, Default)]
pub(crate) struct PackedCrate(pub(crate) Bytes);

impl PackedCrate {
    fn new(value: UnpackedCrate) -> Result<Self, Error> {
        let mut archive = Builder::new(Vec::new());
        archive.mode(HeaderMode::Deterministic);

        let base_path = UnpackedCrate::make_path(&value.tempdir, &value.meta, "");
        archive.append_dir_all(
            format!("{}-{}/", value.meta.name, value.meta.vers),
            base_path,
        )?;
        let data = archive.into_inner()?;

        let reader = Cursor::new(data);
        let mut encoder = GzEncoder::new(reader, Compression::default());
        let mut zipped_data = Vec::new();
        encoder.read_to_end(&mut zipped_data)?;

        Ok(PackedCrate(Bytes::from(zipped_data)))
    }

    fn new_rebased(&self, meta: &PackageMetaData, target_base: &str) -> Result<Self, Error> {
        let mut unpacked = UnpackedCrate::decompress(self.clone(), meta.clone())?;

        let name = Program::program_id_to_crate_name(unpacked.program_id);
        UnpackedCrate::fixup_toml(&unpacked.tempdir, "Cargo.toml.orig", &unpacked.meta, &name)?;
        UnpackedCrate::fixup_toml(&unpacked.tempdir, "Cargo.toml", &unpacked.meta, &name)?;

        let source_path = UnpackedCrate::make_path(&unpacked.tempdir, &unpacked.meta, "");
        unpacked.meta.name = target_base.to_string();
        let target_path = UnpackedCrate::make_path(&unpacked.tempdir, &unpacked.meta, "");
        fs::rename(source_path, target_path.clone())
            .map_err(|_| "Failed to rename the crate folder")?;

        Self::new(unpacked)
    }

    fn version(&self) -> String {
        let decoder = GzDecoder::new(self.0.as_ref());
        let mut archive = Archive::new(decoder);

        if let Some(Ok(entry)) = archive
            .entries()
            .ok()
            .and_then(|mut entries| entries.nth(0))
        {
            if let Ok(path) = entry.path() {
                if let Some(path_str) = path.to_str() {
                    if let Some((_, vers)) = path_str.rsplit_once('-') {
                        let mut version = vers.to_string();
                        // Removing trailing '/'
                        if version.ends_with('/') {
                            version.pop();
                        }
                        return version;
                    }
                }
            }
        }

        // Placeholder version.
        "0.1.0".to_string()
    }
}

pub(crate) struct UnpackedCrate {
    meta: PackageMetaData,
    cksum: String,
    tempdir: Arc<TempDir>,
    program_path: String,
    program_id: Pubkey,
    keypair: Option<Keypair>,
    packed_crate: PackedCrate,
}

impl UnpackedCrate {
    fn decompress(packed_crate: PackedCrate, meta: PackageMetaData) -> Result<Self, Error> {
        let cksum = format!("{:x}", Sha256::digest(&packed_crate.0));

        let decoder = GzDecoder::new(packed_crate.0.as_ref());
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
            packed_crate,
        })
    }

    pub(crate) fn new(bytes: Bytes) -> Result<Self, Error> {
        let (meta, offset) = PackageMetaData::new(&bytes)?;

        let (_crate_file_length, length_size) =
            PackageMetaData::read_u32_length(&bytes.slice(offset..))?;
        let packed_crate = PackedCrate(bytes.slice(offset.saturating_add(length_size)..));
        UnpackedCrate::decompress(packed_crate, meta)
    }

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
        let (packed_crate, meta) = Self::fetch(id, "", client)?;
        let mut entry: IndexEntry = meta.into();
        entry.cksum = format!("{:x}", Sha256::digest(&packed_crate.0));
        Ok(entry)
    }

    pub(crate) fn fetch(
        id: Pubkey,
        vers: &str,
        client: Arc<Client>,
    ) -> Result<(PackedCrate, PackageMetaData), Error> {
        let unpacked = Self::new_empty(id, vers)?;
        let mut program = Program::from(&unpacked);
        program.dump(client)?;

        // Decompile the program
        // Generate a Cargo.toml

        let mut meta = unpacked.meta.clone();

        if APPEND_CRATE_TO_ELF {
            meta.vers = program.packed_crate.version();
            Ok((program.packed_crate, meta))
        } else {
            PackedCrate::new(unpacked).map(|file| (file, meta))
        }
    }

    fn new_empty(id: Pubkey, vers: &str) -> Result<Self, Error> {
        let meta = PackageMetaData {
            name: Program::program_id_to_crate_name(id),
            vers: vers.to_string(),
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

        let base_path = UnpackedCrate::make_path(&tempdir, &meta, "out");
        fs::create_dir_all(base_path)
            .map_err(|_| "Failed to create the base directory for output")?;

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
            packed_crate: PackedCrate::default(),
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

    fn fixup_toml(
        tempdir: &TempDir,
        cargo_toml_name: &str,
        meta: &PackageMetaData,
        name: &str,
    ) -> Result<(), Error> {
        let toml_orig_path = Self::make_path(tempdir, meta, cargo_toml_name);
        let toml_content = fs::read_to_string(&toml_orig_path)?;
        let mut toml = toml_content.parse::<toml::Table>()?;
        toml.get_mut("package")
            .and_then(|v| v.get_mut("name"))
            .map(|v| *v = toml::Value::String(name.to_string()))
            .ok_or("Failed to set package name")?;

        fs::write(toml_orig_path, toml.to_string())?;
        Ok(())
    }
}
