use {
    crate::remote_wallet::{
        RemoteWallet, RemoteWalletError, RemoteWalletInfo, RemoteWalletManager,
    },
    bitcoin::bip32,
    console::Emoji,
    semver::Version as FirmwareVersion,
    solana_sdk::{derivation_path::DerivationPath, pubkey::Pubkey, signature::Signature},
    std::{cell::RefCell, rc::Rc, str::FromStr, fmt},
    trezor_client::{
        client::common::handle_interaction,
        protos::{
            SolanaGetPublicKey,
            SolanaPublicKey,
            SolanaSignTx,
            SolanaTxSignature,
        },
        Trezor,
        utils::convert_path,
    },
};

static CHECK_MARK: Emoji = Emoji("✅ ", "");

/// Trezor Wallet device
pub struct TrezorWallet {
    pub trezor_client: Option<Rc<RefCell<Trezor>>>,
    pub pretty_path: String,
}

impl fmt::Debug for TrezorWallet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "trezor_client")
    }
}

impl TrezorWallet {
    pub fn new(trezor_client: Option<Rc<RefCell<Trezor>>>, pretty_path: String) -> Self {
        Self {
            trezor_client,
            pretty_path,
        }
    }

    pub fn get_trezor_firmware_version(&self) -> Result<FirmwareVersion, RemoteWalletError> {
        match &self.trezor_client {
            Some(trezor_client) => {
                let features = trezor_client.borrow_mut().features().expect("no features").clone();
                Ok(FirmwareVersion::new(features.major_version().into(), features.minor_version().into(), features.patch_version().into()))
            }
            _ => Err(RemoteWalletError::NoDeviceFound)
        }
    }

    pub fn get_trezor_model(&self) -> Result<String, RemoteWalletError> {
        match &self.trezor_client {
            Some(trezor_client) => {
                let features = trezor_client.borrow_mut().features().expect("no features").clone();
                Ok(features.model().to_string())
            }
            _ => Err(RemoteWalletError::NoDeviceFound)
        }
    }

    pub fn get_trezor_device_id(&self) -> Result<String, RemoteWalletError> {
        match &self.trezor_client {
            Some(trezor_client) => {
                let features = trezor_client.borrow_mut().features().expect("no features").clone();
                Ok(features.device_id().to_string())
            }
            _ => Err(RemoteWalletError::NoDeviceFound)
        }
    }
}

impl RemoteWallet<Trezor> for TrezorWallet {
    fn name(&self) -> &str {
        "Trezor hardware wallet"
    }

    /// Parse device info and get device base pubkey
    fn read_device(&mut self, _dev_info: &Trezor) -> Result<RemoteWalletInfo, RemoteWalletError> {
        unimplemented!();
    }

    /// Get solana pubkey from a RemoteWallet
    fn get_pubkey(
        &self,
        derivation_path: &DerivationPath,
        confirm_key: bool,
    ) -> Result<Pubkey, RemoteWalletError> {
        let derivation_path_string = format!("{:?}", derivation_path);
        let derivation_path_str = derivation_path_string.as_str();
        match &self.trezor_client {
            Some(trezor_client) => {
                let mut solana_get_pubkey = SolanaGetPublicKey::new();
                let address_n = convert_path(&bip32::DerivationPath::from_str(derivation_path_str).expect("Should have properly formatted derivation path for converting"));
                solana_get_pubkey.address_n = address_n;
                solana_get_pubkey.show_display = Some(confirm_key);
                if confirm_key {
                    println!("Waiting for your approval on {}", self.name());
                }
                let pubkey = handle_interaction(
                    trezor_client.borrow_mut().call(solana_get_pubkey, Box::new(|_, m: SolanaPublicKey| Ok(m)))?
                )?;
                if confirm_key {
                    println!("{CHECK_MARK}Approved");
                }
                Pubkey::try_from(pubkey.public_key()).map_err(|_| RemoteWalletError::Protocol("Key packet size mismatch"))
            }
            _ => Err(RemoteWalletError::NoDeviceFound)
        }
    }

    /// Sign transaction data with wallet managing pubkey at derivation path
    /// `m/44'/501'/<account>'/<change>'`.
    fn sign_message(
        &self,
        derivation_path: &DerivationPath,
        data: &[u8],
    ) -> Result<Signature, RemoteWalletError> {
        let derivation_path_string = format!("{:?}", derivation_path);
        let derivation_path_str = derivation_path_string.as_str();
        match &self.trezor_client {
            Some(trezor_client) => {
                let mut solana_sign_tx = SolanaSignTx::new();
                let address_n = convert_path(&bip32::DerivationPath::from_str(derivation_path_str).expect("Hardended Derivation Path"));
                solana_sign_tx.address_n = address_n;
                solana_sign_tx.serialized_tx = Some(data.to_vec());
                let solana_tx_signature = handle_interaction(
                    trezor_client.borrow_mut().call(solana_sign_tx, Box::new(|_, m: SolanaTxSignature| Ok(m)))?
                )?;
                Signature::try_from(solana_tx_signature.signature()).map_err(|_e| RemoteWalletError::Protocol("Signature packet size mismatch"))
            }
            _ => Err(RemoteWalletError::NoDeviceFound)
        }
    }

    /// Sign off-chain message with wallet managing pubkey at derivation path
    /// `m/44'/501'/<account>'/<change>'`.
    fn sign_offchain_message(
        &self,
        derivation_path: &DerivationPath,
        message: &[u8],
    ) -> Result<Signature, RemoteWalletError> {
        Self::sign_message(&self, derivation_path, message)
    }
}

pub fn get_trezor_from_info(info: RemoteWalletInfo, _keypair_name: &str, wallet_manager: &RemoteWalletManager,) -> Result<Rc<TrezorWallet>, RemoteWalletError> {
    wallet_manager.get_trezor_wallet(info.get_pretty_path())
}

#[cfg(test)]
mod tests {
    use serial_test::serial;
    use trezor_client::{
        find_devices,
        Model,
    };

    use super::*;

    fn init_emulator() -> Trezor {
        let mut emulator = find_devices(false)
            .into_iter()
            .find(|t| t.model == Model::TrezorEmulator)
            .expect("An emulator should be found")
            .connect()
            .expect("Connection to the emulator should succeed");
        emulator.init_device(None).expect("Initialization of device should succeed");
        emulator
    }

    #[test]
    #[serial]
    fn test_emulator_find() {
        let trezors = find_devices(false);
        assert!(trezors.len() > 0);
        assert!(trezors.iter().any(|t| t.model == Model::TrezorEmulator));
    }

    #[test]
    #[serial]
    fn test_solana_pubkey() {
        let mut emulator = init_emulator();
        let derivation_path_str = "m/44'/501'/0'/0'";
        let mut solana_get_pubkey = SolanaGetPublicKey::new();
        let address_n = convert_path(&bip32::DerivationPath::from_str(derivation_path_str).expect("Failed to parse path"));
        solana_get_pubkey.address_n = address_n;
        solana_get_pubkey.show_display = Some(false);
        let pubkey = handle_interaction(
            emulator.call(solana_get_pubkey, Box::new(|_, m: SolanaPublicKey| Ok(m))).expect("Trezor client (the emulator) has been initialized and SolanaGetPublicKey is initialized correctly")
        ).expect("Trezor client (the emulator) has been initialized and SolanaGetPublicKey is initialized correctly");
        let public_key_string = bitcoin::base58::encode(pubkey.public_key());
        assert!(!public_key_string.is_empty());
    }

    #[test]
    #[serial]
    fn test_trezor_wallet() {
        let emulator = init_emulator();
        let pretty_path = "usb://trezor?key=0/0".to_string();
        let trezor_wallet = TrezorWallet::new(Some(Rc::new(RefCell::new(emulator))), pretty_path);
        let expected_model = "T".to_string();
        let model = trezor_wallet.get_trezor_model().expect("Trezor client (the emulator) has been initialized");
        assert_eq!(expected_model, model);
        let device_id = trezor_wallet.get_trezor_device_id().expect("Trezor client (the emulator) has been initialized");
        assert!(!device_id.is_empty());
        let firmware_version = trezor_wallet.get_trezor_firmware_version();
        assert!(firmware_version.is_ok());
        let derivation_path = DerivationPath::new_bip44(Some(0), Some(0));
        let pubkey = trezor_wallet.get_pubkey(&derivation_path, false).expect("Trezor client (the emulator) has been initialized");
        assert!(!pubkey.to_string().is_empty());
    }

    #[test]
    #[serial]
    fn test_trezor_wallet_with_trezor_client_none() {
        let pretty_path = "usb://trezor?key=0/0".to_string();
        let trezor_wallet = TrezorWallet::new(None, pretty_path);
        let derivation_path = DerivationPath::new_bip44(Some(0), Some(0));
        let res = trezor_wallet.get_pubkey(&derivation_path, false);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err().to_string(), RemoteWalletError::NoDeviceFound.to_string());
    }
}
