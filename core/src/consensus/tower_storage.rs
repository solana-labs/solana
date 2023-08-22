use {
    crate::consensus::{
        tower1_14_11::Tower1_14_11, tower1_7_14::SavedTower1_7_14, Result, Tower, TowerError,
        TowerVersions,
    },
    solana_sdk::{
        pubkey::Pubkey,
        signature::{Signature, Signer},
    },
    std::{
        fs::{self, File},
        io::{self, BufReader},
        path::PathBuf,
    },
};

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq, AbiExample)]
pub enum SavedTowerVersions {
    V1_17_14(SavedTower1_7_14),
    Current(SavedTower),
}

impl SavedTowerVersions {
    fn try_into_tower(&self, node_pubkey: &Pubkey) -> Result<Tower> {
        // This method assumes that `self` was just deserialized
        assert_eq!(self.pubkey(), Pubkey::default());

        let tv = match self {
            SavedTowerVersions::V1_17_14(t) => {
                if !t.signature.verify(node_pubkey.as_ref(), &t.data) {
                    return Err(TowerError::InvalidSignature);
                }
                bincode::deserialize(&t.data).map(TowerVersions::V1_17_14)
            }
            SavedTowerVersions::Current(t) => {
                if !t.signature.verify(node_pubkey.as_ref(), &t.data) {
                    return Err(TowerError::InvalidSignature);
                }
                bincode::deserialize(&t.data).map(TowerVersions::V1_14_11)
            }
        };
        tv.map_err(|e| e.into()).and_then(|tv: TowerVersions| {
            let tower = tv.convert_to_current();
            if tower.node_pubkey != *node_pubkey {
                return Err(TowerError::WrongTower(format!(
                    "node_pubkey is {:?} but found tower for {:?}",
                    node_pubkey, tower.node_pubkey
                )));
            }
            Ok(tower)
        })
    }

    fn serialize_into(&self, file: &mut File) -> Result<()> {
        bincode::serialize_into(file, self).map_err(|e| e.into())
    }

    fn pubkey(&self) -> Pubkey {
        match self {
            SavedTowerVersions::V1_17_14(t) => t.node_pubkey,
            SavedTowerVersions::Current(t) => t.node_pubkey,
        }
    }
}

impl From<SavedTower> for SavedTowerVersions {
    fn from(tower: SavedTower) -> SavedTowerVersions {
        SavedTowerVersions::Current(tower)
    }
}

impl From<SavedTower1_7_14> for SavedTowerVersions {
    fn from(tower: SavedTower1_7_14) -> SavedTowerVersions {
        SavedTowerVersions::V1_17_14(tower)
    }
}

#[frozen_abi(digest = "Gaxfwvx5MArn52mKZQgzHmDCyn5YfCuTHvp5Et3rFfpp")]
#[derive(Default, Clone, Serialize, Deserialize, Debug, PartialEq, Eq, AbiExample)]
pub struct SavedTower {
    signature: Signature,
    data: Vec<u8>,
    #[serde(skip)]
    node_pubkey: Pubkey,
}

impl SavedTower {
    pub fn new<T: Signer>(tower: &Tower, keypair: &T) -> Result<Self> {
        let node_pubkey = keypair.pubkey();
        if tower.node_pubkey != node_pubkey {
            return Err(TowerError::WrongTower(format!(
                "node_pubkey is {:?} but found tower for {:?}",
                node_pubkey, tower.node_pubkey
            )));
        }

        // SavedTower always stores its data in 1_14_11 format
        let tower: Tower1_14_11 = tower.clone().into();

        let data = bincode::serialize(&tower)?;
        let signature = keypair.sign_message(&data);
        Ok(Self {
            signature,
            data,
            node_pubkey,
        })
    }
}

pub trait TowerStorage: Sync + Send {
    fn load(&self, node_pubkey: &Pubkey) -> Result<Tower>;
    fn store(&self, saved_tower: &SavedTowerVersions) -> Result<()>;
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct NullTowerStorage {}

impl TowerStorage for NullTowerStorage {
    fn load(&self, _node_pubkey: &Pubkey) -> Result<Tower> {
        Err(TowerError::IoError(io::Error::new(
            io::ErrorKind::Other,
            "NullTowerStorage::load() not available",
        )))
    }

    fn store(&self, _saved_tower: &SavedTowerVersions) -> Result<()> {
        Ok(())
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FileTowerStorage {
    pub tower_path: PathBuf,
}

impl FileTowerStorage {
    pub fn new(tower_path: PathBuf) -> Self {
        Self { tower_path }
    }

    // Old filename for towers pre 1.9 (VoteStateUpdate)
    pub fn old_filename(&self, node_pubkey: &Pubkey) -> PathBuf {
        self.tower_path
            .join(format!("tower-{node_pubkey}"))
            .with_extension("bin")
    }

    pub fn filename(&self, node_pubkey: &Pubkey) -> PathBuf {
        self.tower_path
            .join(format!("tower-1_9-{node_pubkey}"))
            .with_extension("bin")
    }

    #[cfg(test)]
    fn store_old(&self, saved_tower: &SavedTower1_7_14) -> Result<()> {
        let pubkey = saved_tower.node_pubkey;
        let filename = self.old_filename(&pubkey);
        trace!("store: {}", filename.display());
        let new_filename = filename.with_extension("bin.new");

        {
            // overwrite anything if exists
            let file = File::create(&new_filename)?;
            bincode::serialize_into(file, saved_tower)?;
            // file.sync_all() hurts performance; pipeline sync-ing and submitting votes to the cluster!
        }
        fs::rename(&new_filename, &filename)?;
        // self.path.parent().sync_all() hurts performance same as the above sync
        Ok(())
    }
}

impl TowerStorage for FileTowerStorage {
    fn load(&self, node_pubkey: &Pubkey) -> Result<Tower> {
        let filename = self.filename(node_pubkey);
        trace!("load {}", filename.display());

        // Ensure to create parent dir here, because restore() precedes save() always
        fs::create_dir_all(filename.parent().unwrap())?;

        if let Ok(file) = File::open(&filename) {
            // New format
            let mut stream = BufReader::new(file);

            bincode::deserialize_from(&mut stream)
                .map_err(|e| e.into())
                .and_then(|t: SavedTowerVersions| t.try_into_tower(node_pubkey))
        } else {
            // Old format
            let file = File::open(self.old_filename(node_pubkey))?;
            let mut stream = BufReader::new(file);
            bincode::deserialize_from(&mut stream)
                .map_err(|e| e.into())
                .and_then(|t: SavedTower1_7_14| {
                    SavedTowerVersions::from(t).try_into_tower(node_pubkey)
                })
        }
    }

    fn store(&self, saved_tower: &SavedTowerVersions) -> Result<()> {
        let pubkey = saved_tower.pubkey();
        let filename = self.filename(&pubkey);
        trace!("store: {}", filename.display());
        let new_filename = filename.with_extension("bin.new");

        {
            // overwrite anything if exists
            let mut file = File::create(&new_filename)?;
            saved_tower.serialize_into(&mut file)?;
            // file.sync_all() hurts performance; pipeline sync-ing and submitting votes to the cluster!
        }
        fs::rename(&new_filename, &filename)?;
        // self.path.parent().sync_all() hurts performance same as the above sync
        Ok(())
    }
}

pub struct EtcdTowerStorage {
    client: tokio::sync::Mutex<etcd_client::Client>,
    instance_id: [u8; 8],
    runtime: tokio::runtime::Runtime,
}

pub struct EtcdTlsConfig {
    pub domain_name: String,
    pub ca_certificate: Vec<u8>,
    pub identity_certificate: Vec<u8>,
    pub identity_private_key: Vec<u8>,
}

impl EtcdTowerStorage {
    pub fn new<E: AsRef<str>, S: AsRef<[E]>>(
        endpoints: S,
        tls_config: Option<EtcdTlsConfig>,
    ) -> Result<Self> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .unwrap();

        let client = runtime
            .block_on(etcd_client::Client::connect(
                endpoints,
                tls_config.map(|tls_config| {
                    etcd_client::ConnectOptions::default().with_tls(
                        etcd_client::TlsOptions::new()
                            .domain_name(tls_config.domain_name)
                            .ca_certificate(etcd_client::Certificate::from_pem(
                                tls_config.ca_certificate,
                            ))
                            .identity(etcd_client::Identity::from_pem(
                                tls_config.identity_certificate,
                                tls_config.identity_private_key,
                            )),
                    )
                }),
            ))
            .map_err(Self::etdc_to_tower_error)?;

        Ok(Self {
            client: tokio::sync::Mutex::new(client),
            instance_id: solana_sdk::timing::timestamp().to_le_bytes(),
            runtime,
        })
    }

    fn get_keys(node_pubkey: &Pubkey) -> (String, String) {
        let instance_key = format!("{node_pubkey}/instance");
        let tower_key = format!("{node_pubkey}/tower");
        (instance_key, tower_key)
    }

    fn etdc_to_tower_error(error: etcd_client::Error) -> TowerError {
        TowerError::IoError(io::Error::new(io::ErrorKind::Other, error.to_string()))
    }
}

impl TowerStorage for EtcdTowerStorage {
    fn load(&self, node_pubkey: &Pubkey) -> Result<Tower> {
        let (instance_key, tower_key) = Self::get_keys(node_pubkey);

        let txn = etcd_client::Txn::new().and_then(vec![etcd_client::TxnOp::put(
            instance_key.clone(),
            self.instance_id,
            None,
        )]);
        self.runtime
            .block_on(async { self.client.lock().await.txn(txn).await })
            .map_err(|err| {
                error!("Failed to acquire etcd instance lock: {}", err);
                Self::etdc_to_tower_error(err)
            })?;

        let txn = etcd_client::Txn::new()
            .when(vec![etcd_client::Compare::value(
                instance_key,
                etcd_client::CompareOp::Equal,
                self.instance_id,
            )])
            .and_then(vec![etcd_client::TxnOp::get(tower_key, None)]);

        let response = self
            .runtime
            .block_on(async { self.client.lock().await.txn(txn).await })
            .map_err(|err| {
                error!("Failed to read etcd saved tower: {}", err);
                Self::etdc_to_tower_error(err)
            })?;

        if !response.succeeded() {
            return Err(TowerError::IoError(io::Error::new(
                io::ErrorKind::Other,
                format!("Lost etcd instance lock for {node_pubkey}"),
            )));
        }

        for op_response in response.op_responses() {
            if let etcd_client::TxnOpResponse::Get(get_response) = op_response {
                if let Some(kv) = get_response.kvs().get(0) {
                    return bincode::deserialize_from(kv.value())
                        .map_err(|e| e.into())
                        .and_then(|t: SavedTowerVersions| t.try_into_tower(node_pubkey));
                }
            }
        }

        // Should never happen...
        Err(TowerError::IoError(io::Error::new(
            io::ErrorKind::Other,
            "Saved tower response missing".to_string(),
        )))
    }

    fn store(&self, saved_tower: &SavedTowerVersions) -> Result<()> {
        let (instance_key, tower_key) = Self::get_keys(&saved_tower.pubkey());

        let txn = etcd_client::Txn::new()
            .when(vec![etcd_client::Compare::value(
                instance_key,
                etcd_client::CompareOp::Equal,
                self.instance_id,
            )])
            .and_then(vec![etcd_client::TxnOp::put(
                tower_key,
                bincode::serialize(&saved_tower)?,
                None,
            )]);

        let response = self
            .runtime
            .block_on(async { self.client.lock().await.txn(txn).await })
            .map_err(|err| {
                error!("Failed to write etcd saved tower: {}", err);
                err
            })
            .map_err(Self::etdc_to_tower_error)?;

        if !response.succeeded() {
            return Err(TowerError::IoError(io::Error::new(
                io::ErrorKind::Other,
                format!("Lost etcd instance lock for {}", saved_tower.pubkey()),
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
pub mod test {
    use {
        super::*,
        crate::consensus::{
            tower1_7_14::{SavedTower1_7_14, Tower1_7_14},
            Tower,
        },
        solana_sdk::{hash::Hash, signature::Keypair},
        solana_vote_program::vote_state::{
            BlockTimestamp, LandedVote, Vote, VoteState, VoteState1_14_11, VoteTransaction,
            MAX_LOCKOUT_HISTORY,
        },
        tempfile::TempDir,
    };

    #[test]
    fn test_tower_migration() {
        let tower_path = TempDir::new().unwrap();
        let identity_keypair = Keypair::new();
        let node_pubkey = identity_keypair.pubkey();
        let mut vote_state = VoteState::default();
        vote_state
            .votes
            .resize(MAX_LOCKOUT_HISTORY, LandedVote::default());
        vote_state.root_slot = Some(1);

        let vote = Vote::new(vec![1, 2, 3, 4], Hash::default());
        let tower_storage = FileTowerStorage::new(tower_path.path().to_path_buf());

        let old_tower = Tower1_7_14 {
            node_pubkey,
            threshold_depth: 10,
            threshold_size: 0.9,
            vote_state: VoteState1_14_11::from(vote_state),
            last_vote: vote.clone(),
            last_timestamp: BlockTimestamp::default(),
            last_vote_tx_blockhash: None,
            stray_restored_slot: Some(2),
            last_switch_threshold_check: Option::default(),
        };

        {
            let saved_tower = SavedTower1_7_14::new(&old_tower, &identity_keypair).unwrap();
            tower_storage.store_old(&saved_tower).unwrap();
        }

        let loaded = Tower::restore(&tower_storage, &node_pubkey).unwrap();
        assert_eq!(loaded.node_pubkey, old_tower.node_pubkey);
        assert_eq!(loaded.last_vote(), VoteTransaction::from(vote));
        assert_eq!(loaded.vote_state.root_slot, Some(1));
        assert_eq!(loaded.stray_restored_slot(), None);
    }
}
