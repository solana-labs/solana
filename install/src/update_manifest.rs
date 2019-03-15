use serde_derive::{Deserialize, Serialize};
use solana_sdk::signature::Signature;

/// Information required to download and apply a given update
#[derive(Serialize, Deserialize, Default, Debug, PartialEq)]
pub struct UpdateManifest {
    pub target: String,                // Target triple (TARGET)
    pub commit: String, // git sha1 of this update, must match the commit sha1 in the release tar.bz2
    pub timestamp_secs: u64, // When the release was deployed in seconds since UNIX EPOCH
    pub download_url: String, // Download URL to the release tar.bz2
    pub download_signature: Signature, // Signature of the release tar.bz2 file, verify with the Account public key
}

/// Userdata of an Update Manifest program Account.
#[derive(Serialize, Deserialize, Default, Debug, PartialEq)]
pub struct SignedUpdateManifest {
    pub manifest: UpdateManifest,
    pub manifest_signature: Signature, // Signature of UpdateInfo, verify with the Account public key
}
