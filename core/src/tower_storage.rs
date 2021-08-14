use {
    crate::consensus::{Result, Tower, TowerError},
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

#[frozen_abi(digest = "Gaxfwvx5MArn52mKZQgzHmDCyn5YfCuTHvp5Et3rFfpp")]
#[derive(Default, Clone, Serialize, Deserialize, Debug, PartialEq, AbiExample)]
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

        let data = bincode::serialize(tower)?;
        let signature = keypair.sign_message(&data);
        Ok(Self {
            signature,
            data,
            node_pubkey,
        })
    }

    pub fn try_into_tower(self, node_pubkey: &Pubkey) -> Result<Tower> {
        // This method assumes that `self` was just deserialized
        assert_eq!(self.node_pubkey, Pubkey::default());

        if !self.signature.verify(node_pubkey.as_ref(), &self.data) {
            return Err(TowerError::InvalidSignature);
        }
        bincode::deserialize(&self.data)
            .map_err(|e| e.into())
            .and_then(|tower: Tower| {
                if tower.node_pubkey != *node_pubkey {
                    return Err(TowerError::WrongTower(format!(
                        "node_pubkey is {:?} but found tower for {:?}",
                        node_pubkey, tower.node_pubkey
                    )));
                }
                Ok(tower)
            })
    }
}

pub trait TowerStorage: Sync + Send {
    fn load(&self, node_pubkey: &Pubkey) -> Result<SavedTower>;
    fn store(&self, saved_tower: &SavedTower) -> Result<()>;
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct NullTowerStorage {}

impl TowerStorage for NullTowerStorage {
    fn load(&self, _node_pubkey: &Pubkey) -> Result<SavedTower> {
        Err(TowerError::IoError(io::Error::new(
            io::ErrorKind::Other,
            "NullTowerStorage::load() not available",
        )))
    }

    fn store(&self, _saved_tower: &SavedTower) -> Result<()> {
        Ok(())
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct FileTowerStorage {
    pub tower_path: PathBuf,
}

impl FileTowerStorage {
    pub fn new(tower_path: PathBuf) -> Self {
        Self { tower_path }
    }

    pub fn filename(&self, node_pubkey: &Pubkey) -> PathBuf {
        self.tower_path
            .join(format!("tower-{}", node_pubkey))
            .with_extension("bin")
    }
}

impl TowerStorage for FileTowerStorage {
    fn load(&self, node_pubkey: &Pubkey) -> Result<SavedTower> {
        let filename = self.filename(node_pubkey);
        trace!("load {}", filename.display());

        // Ensure to create parent dir here, because restore() precedes save() always
        fs::create_dir_all(&filename.parent().unwrap())?;

        let file = File::open(&filename)?;
        let mut stream = BufReader::new(file);
        bincode::deserialize_from(&mut stream).map_err(|e| e.into())
    }

    fn store(&self, saved_tower: &SavedTower) -> Result<()> {
        let filename = self.filename(&saved_tower.node_pubkey);
        trace!("store: {}", filename.display());
        let new_filename = filename.with_extension("bin.new");

        {
            // overwrite anything if exists
            let mut file = File::create(&new_filename)?;
            bincode::serialize_into(&mut file, saved_tower)?;
            // file.sync_all() hurts performance; pipeline sync-ing and submitting votes to the cluster!
        }
        fs::rename(&new_filename, &filename)?;
        // self.path.parent().sync_all() hurts performance same as the above sync
        Ok(())
    }
}
