use crate::remote_wallet::{DerivationPath, RemoteWallet, RemoteWalletType};
use solana_sdk::{
    pubkey::Pubkey,
    signature::{KeypairUtil, Signature},
};
use std::error;

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

impl KeypairUtil for RemoteKeypair {
    fn try_pubkey(&self) -> Result<Pubkey, Box<dyn error::Error>> {
        match &self.wallet_type {
            RemoteWalletType::Ledger(wallet) => wallet
                .get_pubkey(&self.derivation_path)
                .map_err(|e| e.into()),
        }
    }

    fn try_sign_message(&self, message: &[u8]) -> Result<Signature, Box<dyn error::Error>> {
        match &self.wallet_type {
            RemoteWalletType::Ledger(wallet) => wallet
                .sign_message(&self.derivation_path, message)
                .map_err(|e| e.into()),
        }
    }
}
