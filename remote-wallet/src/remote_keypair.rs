use crate::{
    ledger::get_ledger_from_info,
    remote_wallet::{
        DerivationPath, RemoteWallet, RemoteWalletError, RemoteWalletInfo, RemoteWalletType,
    },
};
use solana_sdk::{
    pubkey::Pubkey,
    signature::{Signature, Signer, SignerError},
};

pub struct RemoteKeypair {
    pub wallet_type: RemoteWalletType,
    pub derivation_path: DerivationPath,
}

impl RemoteKeypair {
    pub fn new(wallet_type: RemoteWalletType, derivation_path: DerivationPath) -> Self {
        Self {
            wallet_type,
            derivation_path,
        }
    }
}

impl Signer for RemoteKeypair {
    fn try_pubkey(&self) -> Result<Pubkey, SignerError> {
        match &self.wallet_type {
            RemoteWalletType::Ledger(wallet) => wallet
                .get_pubkey(&self.derivation_path)
                .map_err(|e| e.into()),
        }
    }

    fn try_sign_message(&self, message: &[u8]) -> Result<Signature, SignerError> {
        match &self.wallet_type {
            RemoteWalletType::Ledger(wallet) => wallet
                .sign_message(&self.derivation_path, message)
                .map_err(|e| e.into()),
        }
    }
}

pub fn generate_remote_keypair(
    path: String,
    explicit_derivation_path: Option<DerivationPath>,
) -> Result<RemoteKeypair, RemoteWalletError> {
    let (remote_wallet_info, mut derivation_path) = RemoteWalletInfo::parse_path(path)?;
    if let Some(derivation) = explicit_derivation_path {
        derivation_path = derivation;
    }
    if remote_wallet_info.manufacturer == "ledger" {
        let ledger = get_ledger_from_info(remote_wallet_info)?;
        Ok(RemoteKeypair {
            wallet_type: RemoteWalletType::Ledger(ledger),
            derivation_path,
        })
    } else {
        Err(RemoteWalletError::DeviceTypeMismatch)
    }
}
