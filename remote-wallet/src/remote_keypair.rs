use crate::{
    ledger::get_ledger_from_info,
    remote_wallet::{
        DerivationPath, RemoteWallet, RemoteWalletError, RemoteWalletInfo, RemoteWalletManager,
        RemoteWalletType,
    },
};
use solana_sdk::{
    pubkey::Pubkey,
    signature::{Signature, Signer, SignerError},
};

pub struct RemoteKeypair {
    pub wallet_type: RemoteWalletType,
    pub derivation_path: DerivationPath,
    pub pubkey: Pubkey,
}

impl RemoteKeypair {
    pub fn new(
        wallet_type: RemoteWalletType,
        derivation_path: DerivationPath,
        confirm_key: bool,
    ) -> Result<Self, RemoteWalletError> {
        let pubkey = match &wallet_type {
            RemoteWalletType::Ledger(wallet) => wallet.get_pubkey(&derivation_path, confirm_key)?,
        };

        Ok(Self {
            wallet_type,
            derivation_path,
            pubkey,
        })
    }
}

impl Signer for RemoteKeypair {
    fn try_pubkey(&self) -> Result<Pubkey, SignerError> {
        Ok(self.pubkey)
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
    wallet_manager: &RemoteWalletManager,
    confirm_key: bool,
    keypair_name: &str,
) -> Result<RemoteKeypair, RemoteWalletError> {
    let (remote_wallet_info, derivation_path) = RemoteWalletInfo::parse_path(path)?;
    if remote_wallet_info.manufacturer == "ledger" {
        let ledger = get_ledger_from_info(remote_wallet_info, keypair_name, wallet_manager)?;
        Ok(RemoteKeypair::new(
            RemoteWalletType::Ledger(ledger),
            derivation_path,
            confirm_key,
        )?)
    } else {
        Err(RemoteWalletError::DeviceTypeMismatch)
    }
}
