#[cfg(feature = "hidapi")]
use {crate::ledger::is_valid_ledger, parking_lot::Mutex, std::sync::Arc};
use {
    crate::{
        ledger::LedgerWallet,
        ledger_error::LedgerError,
        locator::{Locator, LocatorError, Manufacturer},
    },
    log::*,
    parking_lot::RwLock,
    solana_sdk::{
        derivation_path::{DerivationPath, DerivationPathError},
        pubkey::Pubkey,
        signature::{Signature, SignerError},
    },
    std::{
        rc::Rc,
        time::{Duration, Instant},
    },
    thiserror::Error,
};

const HID_GLOBAL_USAGE_PAGE: u16 = 0xFF00;
const HID_USB_DEVICE_CLASS: u8 = 0;

/// Remote wallet error.
#[derive(Error, Debug, Clone)]
pub enum RemoteWalletError {
    #[error("hidapi error")]
    Hid(String),

    #[error("device type mismatch")]
    DeviceTypeMismatch,

    #[error("device with non-supported product ID or vendor ID was detected")]
    InvalidDevice,

    #[error(transparent)]
    DerivationPathError(#[from] DerivationPathError),

    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("invalid path: {0}")]
    InvalidPath(String),

    #[error(transparent)]
    LedgerError(#[from] LedgerError),

    #[error("no device found")]
    NoDeviceFound,

    #[error("protocol error: {0}")]
    Protocol(&'static str),

    #[error("pubkey not found for given address")]
    PubkeyNotFound,

    #[error("remote wallet operation rejected by the user")]
    UserCancel,

    #[error(transparent)]
    LocatorError(#[from] LocatorError),
}

#[cfg(feature = "hidapi")]
impl From<hidapi::HidError> for RemoteWalletError {
    fn from(err: hidapi::HidError) -> RemoteWalletError {
        RemoteWalletError::Hid(err.to_string())
    }
}

impl From<RemoteWalletError> for SignerError {
    fn from(err: RemoteWalletError) -> SignerError {
        match err {
            RemoteWalletError::Hid(hid_error) => SignerError::Connection(hid_error),
            RemoteWalletError::DeviceTypeMismatch => SignerError::Connection(err.to_string()),
            RemoteWalletError::InvalidDevice => SignerError::Connection(err.to_string()),
            RemoteWalletError::InvalidInput(input) => SignerError::InvalidInput(input),
            RemoteWalletError::LedgerError(e) => SignerError::Protocol(e.to_string()),
            RemoteWalletError::NoDeviceFound => SignerError::NoDeviceFound,
            RemoteWalletError::Protocol(e) => SignerError::Protocol(e.to_string()),
            RemoteWalletError::UserCancel => {
                SignerError::UserCancel("remote wallet operation rejected by the user".to_string())
            }
            _ => SignerError::Custom(err.to_string()),
        }
    }
}

/// Collection of connected RemoteWallets
pub struct RemoteWalletManager {
    #[cfg(feature = "hidapi")]
    usb: Arc<Mutex<hidapi::HidApi>>,
    devices: RwLock<Vec<Device>>,
}

impl RemoteWalletManager {
    /// Create a new instance.
    #[cfg(feature = "hidapi")]
    pub fn new(usb: Arc<Mutex<hidapi::HidApi>>) -> Rc<Self> {
        Rc::new(Self {
            usb,
            devices: RwLock::new(Vec::new()),
        })
    }

    /// Repopulate device list
    /// Note: this method iterates over and updates all devices
    #[cfg(feature = "hidapi")]
    pub fn update_devices(&self) -> Result<usize, RemoteWalletError> {
        let mut usb = self.usb.lock();
        usb.refresh_devices()?;
        let devices = usb.device_list();
        let num_prev_devices = self.devices.read().len();

        let mut detected_devices = vec![];
        let mut errors = vec![];
        for device_info in devices.filter(|&device_info| {
            is_valid_hid_device(device_info.usage_page(), device_info.interface_number())
                && is_valid_ledger(device_info.vendor_id(), device_info.product_id())
        }) {
            match usb.open_path(device_info.path()) {
                Ok(device) => {
                    let mut ledger = LedgerWallet::new(device);
                    let result = ledger.read_device(device_info);
                    match result {
                        Ok(info) => {
                            ledger.pretty_path = info.get_pretty_path();
                            let path = device_info.path().to_str().unwrap().to_string();
                            trace!("Found device: {:?}", info);
                            detected_devices.push(Device {
                                path,
                                info,
                                wallet_type: RemoteWalletType::Ledger(Rc::new(ledger)),
                            })
                        }
                        Err(err) => {
                            error!("Error connecting to ledger device to read info: {}", err);
                            errors.push(err)
                        }
                    }
                }
                Err(err) => error!("Error connecting to ledger device to read info: {}", err),
            }
        }

        let num_curr_devices = detected_devices.len();
        *self.devices.write() = detected_devices;

        if num_curr_devices == 0 && !errors.is_empty() {
            return Err(errors[0].clone());
        }

        Ok(num_curr_devices - num_prev_devices)
    }

    #[cfg(not(feature = "hidapi"))]
    pub fn update_devices(&self) -> Result<usize, RemoteWalletError> {
        Err(RemoteWalletError::Hid(
            "hidapi crate compilation disabled in solana-remote-wallet.".to_string(),
        ))
    }

    /// List connected and acknowledged wallets
    pub fn list_devices(&self) -> Vec<RemoteWalletInfo> {
        self.devices.read().iter().map(|d| d.info.clone()).collect()
    }

    /// Get a particular wallet
    #[allow(unreachable_patterns)]
    pub fn get_ledger(
        &self,
        host_device_path: &str,
    ) -> Result<Rc<LedgerWallet>, RemoteWalletError> {
        self.devices
            .read()
            .iter()
            .find(|device| device.info.host_device_path == host_device_path)
            .ok_or(RemoteWalletError::PubkeyNotFound)
            .and_then(|device| match &device.wallet_type {
                RemoteWalletType::Ledger(ledger) => Ok(ledger.clone()),
                _ => Err(RemoteWalletError::DeviceTypeMismatch),
            })
    }

    /// Get wallet info.
    pub fn get_wallet_info(&self, pubkey: &Pubkey) -> Option<RemoteWalletInfo> {
        self.devices
            .read()
            .iter()
            .find(|d| &d.info.pubkey == pubkey)
            .map(|d| d.info.clone())
    }

    /// Update devices in maximum `max_polling_duration` if it doesn't succeed
    pub fn try_connect_polling(&self, max_polling_duration: &Duration) -> bool {
        let start_time = Instant::now();
        while start_time.elapsed() <= *max_polling_duration {
            if let Ok(num_devices) = self.update_devices() {
                let plural = if num_devices == 1 { "" } else { "s" };
                trace!("{} Remote Wallet{} found", num_devices, plural);
                return true;
            }
        }
        false
    }
}

/// `RemoteWallet` trait
#[allow(unused_variables)]
pub trait RemoteWallet<T> {
    fn name(&self) -> &str {
        "unimplemented"
    }

    /// Parse device info and get device base pubkey
    fn read_device(&mut self, dev_info: &T) -> Result<RemoteWalletInfo, RemoteWalletError> {
        unimplemented!();
    }

    /// Get solana pubkey from a RemoteWallet
    fn get_pubkey(
        &self,
        derivation_path: &DerivationPath,
        confirm_key: bool,
    ) -> Result<Pubkey, RemoteWalletError> {
        unimplemented!();
    }

    /// Sign transaction data with wallet managing pubkey at derivation path
    /// `m/44'/501'/<account>'/<change>'`.
    fn sign_message(
        &self,
        derivation_path: &DerivationPath,
        data: &[u8],
    ) -> Result<Signature, RemoteWalletError> {
        unimplemented!();
    }

    /// Sign off-chain message with wallet managing pubkey at derivation path
    /// `m/44'/501'/<account>'/<change>'`.
    fn sign_offchain_message(
        &self,
        derivation_path: &DerivationPath,
        message: &[u8],
    ) -> Result<Signature, RemoteWalletError> {
        unimplemented!();
    }
}

/// `RemoteWallet` device
#[derive(Debug)]
pub struct Device {
    #[allow(dead_code)]
    pub(crate) path: String,
    pub(crate) info: RemoteWalletInfo,
    pub wallet_type: RemoteWalletType,
}

/// Remote wallet convenience enum to hold various wallet types
#[derive(Debug)]
pub enum RemoteWalletType {
    Ledger(Rc<LedgerWallet>),
}

/// Remote wallet information.
#[derive(Debug, Default, Clone)]
pub struct RemoteWalletInfo {
    /// RemoteWallet device model
    pub model: String,
    /// RemoteWallet device manufacturer
    pub manufacturer: Manufacturer,
    /// RemoteWallet device serial number
    pub serial: String,
    /// RemoteWallet host device path
    pub host_device_path: String,
    /// Base pubkey of device at Solana derivation path
    pub pubkey: Pubkey,
    /// Initial read error
    pub error: Option<RemoteWalletError>,
}

impl RemoteWalletInfo {
    pub fn parse_locator(locator: Locator) -> Self {
        RemoteWalletInfo {
            manufacturer: locator.manufacturer,
            pubkey: locator.pubkey.unwrap_or_default(),
            ..RemoteWalletInfo::default()
        }
    }

    pub fn get_pretty_path(&self) -> String {
        format!("usb://{}/{:?}", self.manufacturer, self.pubkey,)
    }

    pub(crate) fn matches(&self, other: &Self) -> bool {
        self.manufacturer == other.manufacturer
            && (self.pubkey == other.pubkey
                || self.pubkey == Pubkey::default()
                || other.pubkey == Pubkey::default())
    }
}

/// Helper to determine if a device is a valid HID
pub fn is_valid_hid_device(usage_page: u16, interface_number: i32) -> bool {
    usage_page == HID_GLOBAL_USAGE_PAGE || interface_number == HID_USB_DEVICE_CLASS as i32
}

/// Helper to initialize hidapi and RemoteWalletManager
#[cfg(feature = "hidapi")]
pub fn initialize_wallet_manager() -> Result<Rc<RemoteWalletManager>, RemoteWalletError> {
    let hidapi = Arc::new(Mutex::new(hidapi::HidApi::new()?));
    Ok(RemoteWalletManager::new(hidapi))
}
#[cfg(not(feature = "hidapi"))]
pub fn initialize_wallet_manager() -> Result<Rc<RemoteWalletManager>, RemoteWalletError> {
    Err(RemoteWalletError::Hid(
        "hidapi crate compilation disabled in solana-remote-wallet.".to_string(),
    ))
}

pub fn maybe_wallet_manager() -> Result<Option<Rc<RemoteWalletManager>>, RemoteWalletError> {
    let wallet_manager = initialize_wallet_manager()?;
    let device_count = wallet_manager.update_devices()?;
    if device_count > 0 {
        Ok(Some(wallet_manager))
    } else {
        drop(wallet_manager);
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_locator() {
        let pubkey = solana_sdk::pubkey::new_rand();
        let locator = Locator {
            manufacturer: Manufacturer::Ledger,
            pubkey: Some(pubkey),
        };
        let wallet_info = RemoteWalletInfo::parse_locator(locator);
        assert!(wallet_info.matches(&RemoteWalletInfo {
            model: "nano-s".to_string(),
            manufacturer: Manufacturer::Ledger,
            serial: "".to_string(),
            host_device_path: "/host/device/path".to_string(),
            pubkey,
            error: None,
        }));

        // Test that pubkey need not be populated
        let locator = Locator {
            manufacturer: Manufacturer::Ledger,
            pubkey: None,
        };
        let wallet_info = RemoteWalletInfo::parse_locator(locator);
        assert!(wallet_info.matches(&RemoteWalletInfo {
            model: "nano-s".to_string(),
            manufacturer: Manufacturer::Ledger,
            serial: "".to_string(),
            host_device_path: "/host/device/path".to_string(),
            pubkey: Pubkey::default(),
            error: None,
        }));
    }

    #[test]
    fn test_remote_wallet_info_matches() {
        let pubkey = solana_sdk::pubkey::new_rand();
        let info = RemoteWalletInfo {
            manufacturer: Manufacturer::Ledger,
            model: "Nano S".to_string(),
            serial: "0001".to_string(),
            host_device_path: "/host/device/path".to_string(),
            pubkey,
            error: None,
        };
        let mut test_info = RemoteWalletInfo {
            manufacturer: Manufacturer::Unknown,
            ..RemoteWalletInfo::default()
        };
        assert!(!info.matches(&test_info));
        test_info.manufacturer = Manufacturer::Ledger;
        assert!(info.matches(&test_info));
        test_info.model = "Other".to_string();
        assert!(info.matches(&test_info));
        test_info.model = "Nano S".to_string();
        assert!(info.matches(&test_info));
        test_info.host_device_path = "/host/device/path".to_string();
        assert!(info.matches(&test_info));
        let another_pubkey = solana_sdk::pubkey::new_rand();
        test_info.pubkey = another_pubkey;
        assert!(!info.matches(&test_info));
        test_info.pubkey = pubkey;
        assert!(info.matches(&test_info));
    }

    #[test]
    fn test_get_pretty_path() {
        let pubkey = solana_sdk::pubkey::new_rand();
        let pubkey_str = pubkey.to_string();
        let remote_wallet_info = RemoteWalletInfo {
            model: "nano-s".to_string(),
            manufacturer: Manufacturer::Ledger,
            serial: "".to_string(),
            host_device_path: "/host/device/path".to_string(),
            pubkey,
            error: None,
        };
        assert_eq!(
            remote_wallet_info.get_pretty_path(),
            format!("usb://ledger/{pubkey_str}")
        );
    }
}
