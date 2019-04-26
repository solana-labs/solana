use serde_derive::{Deserialize, Serialize};
use solana_config_api::ConfigState;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Signable, Signature};
use std::error;
use std::io;

/// Information required to download and apply a given update
#[derive(Serialize, Deserialize, Default, Debug, PartialEq)]
pub struct UpdateManifest {
    pub timestamp_secs: u64, // When the release was deployed in seconds since UNIX EPOCH
    pub download_url: String, // Download URL to the release tar.bz2
    pub download_sha256: String, // SHA256 digest of the release tar.bz2 file
}

/// Userdata of an Update Manifest program Account.
#[derive(Serialize, Deserialize, Default, Debug, PartialEq)]
pub struct SignedUpdateManifest {
    pub manifest: UpdateManifest,
    pub manifest_signature: Signature,
    #[serde(skip)]
    pub account_pubkey: Pubkey,
}

impl Signable for SignedUpdateManifest {
    fn pubkey(&self) -> Pubkey {
        self.account_pubkey
    }

    fn signable_data(&self) -> Vec<u8> {
        bincode::serialize(&self.manifest).expect("serialize")
    }
    fn get_signature(&self) -> Signature {
        self.manifest_signature
    }
    fn set_signature(&mut self, signature: Signature) {
        self.manifest_signature = signature
    }
}

impl SignedUpdateManifest {
    pub fn deserialize(account_pubkey: &Pubkey, input: &[u8]) -> Result<Self, Box<error::Error>> {
        let mut manifest: SignedUpdateManifest = bincode::deserialize(input)?;
        manifest.account_pubkey = *account_pubkey;
        if !manifest.verify() {
            Err(io::Error::new(
                io::ErrorKind::Other,
                "Manifest failed to verify",
            ))?;
        }
        Ok(manifest)
    }
}

impl ConfigState for SignedUpdateManifest {
    fn max_space() -> u64 {
        // TODO: Use a fully populated manifest to compute a better value
        //      bincode::serialized_size(&Self::default()).unwrap()
        256
    }
}
