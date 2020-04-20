use crate::ledger::{is_valid_ledger, LedgerWallet};
use log::*;
use parking_lot::{Mutex, RwLock};
use solana_sdk::{
    pubkey::Pubkey,
    signature::{Signature, SignerError},
};
use std::{
    fmt,
    str::FromStr,
    sync::Arc,
    time::{Duration, Instant},
};
use thiserror::Error;
use url::Url;

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

    #[error("invalid derivation path: {0}")]
    InvalidDerivationPath(String),

    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("invalid path: {0}")]
    InvalidPath(String),

    #[error("no device found")]
    NoDeviceFound,

    #[error("protocol error: {0}")]
    Protocol(&'static str),

    #[error("pubkey not found for given address")]
    PubkeyNotFound,

    #[error("remote wallet operation rejected by the user")]
    UserCancel,
}

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
            RemoteWalletError::NoDeviceFound => SignerError::NoDeviceFound,
            RemoteWalletError::Protocol(e) => SignerError::Protocol(e.to_string()),
            RemoteWalletError::UserCancel => {
                SignerError::UserCancel("remote wallet operation rejected by the user".to_string())
            }
            _ => SignerError::Custom(err.to_string()),
        }
    }
}

/// Collection of conntected RemoteWallets
pub struct RemoteWalletManager {
    usb: Arc<Mutex<hidapi::HidApi>>,
    devices: RwLock<Vec<Device>>,
}

impl RemoteWalletManager {
    /// Create a new instance.
    pub fn new(usb: Arc<Mutex<hidapi::HidApi>>) -> Arc<Self> {
        Arc::new(Self {
            usb,
            devices: RwLock::new(Vec::new()),
        })
    }

    /// Repopulate device list
    /// Note: this method iterates over and updates all devices
    pub fn update_devices(&self) -> Result<usize, RemoteWalletError> {
        let mut usb = self.usb.lock();
        usb.refresh_devices()?;
        let devices = usb.device_list();
        let num_prev_devices = self.devices.read().len();

        let detected_devices = devices
            .filter(|&device_info| {
                is_valid_hid_device(device_info.usage_page(), device_info.interface_number())
            })
            .fold(Vec::new(), |mut v, device_info| {
                if is_valid_ledger(device_info.vendor_id(), device_info.product_id()) {
                    match usb.open_path(&device_info.path()) {
                        Ok(device) => {
                            let mut ledger = LedgerWallet::new(device);
                            if let Ok(info) = ledger.read_device(&device_info) {
                                ledger.pretty_path = info.get_pretty_path();
                                let path = device_info.path().to_str().unwrap().to_string();
                                trace!("Found device: {:?}", info);
                                v.push(Device {
                                    path,
                                    info,
                                    wallet_type: RemoteWalletType::Ledger(Arc::new(ledger)),
                                })
                            }
                        }
                        Err(e) => error!("Error connecting to ledger device to read info: {}", e),
                    }
                }
                v
            });

        let num_curr_devices = detected_devices.len();
        *self.devices.write() = detected_devices;

        Ok(num_curr_devices - num_prev_devices)
    }

    /// List connected and acknowledged wallets
    pub fn list_devices(&self) -> Vec<RemoteWalletInfo> {
        self.devices.read().iter().map(|d| d.info.clone()).collect()
    }

    /// Get a particular wallet
    #[allow(unreachable_patterns)]
    pub fn get_ledger(&self, pubkey: &Pubkey) -> Result<Arc<LedgerWallet>, RemoteWalletError> {
        self.devices
            .read()
            .iter()
            .find(|device| &device.info.pubkey == pubkey)
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
pub trait RemoteWallet {
    fn name(&self) -> &str {
        "remote wallet"
    }

    /// Parse device info and get device base pubkey
    fn read_device(
        &mut self,
        dev_info: &hidapi::DeviceInfo,
    ) -> Result<RemoteWalletInfo, RemoteWalletError>;

    /// Get solana pubkey from a RemoteWallet
    fn get_pubkey(
        &self,
        derivation_path: &DerivationPath,
        confirm_key: bool,
    ) -> Result<Pubkey, RemoteWalletError>;

    /// Sign transaction data with wallet managing pubkey at derivation path m/44'/501'/<account>'/<change>'.
    fn sign_message(
        &self,
        derivation_path: &DerivationPath,
        data: &[u8],
    ) -> Result<Signature, RemoteWalletError>;
}

/// `RemoteWallet` device
#[derive(Debug)]
pub struct Device {
    pub(crate) path: String,
    pub(crate) info: RemoteWalletInfo,
    pub wallet_type: RemoteWalletType,
}

/// Remote wallet convenience enum to hold various wallet types
#[derive(Debug)]
pub enum RemoteWalletType {
    Ledger(Arc<LedgerWallet>),
}

/// Remote wallet information.
#[derive(Debug, Default, Clone)]
pub struct RemoteWalletInfo {
    /// RemoteWallet device model
    pub model: String,
    /// RemoteWallet device manufacturer
    pub manufacturer: String,
    /// RemoteWallet device serial number
    pub serial: String,
    /// Base pubkey of device at Solana derivation path
    pub pubkey: Pubkey,
    /// Initial read error
    pub error: Option<RemoteWalletError>,
}

impl RemoteWalletInfo {
    pub fn parse_path(path: String) -> Result<(Self, DerivationPath), RemoteWalletError> {
        let wallet_path = Url::parse(&path).map_err(|e| {
            RemoteWalletError::InvalidDerivationPath(format!("parse error: {:?}", e))
        })?;

        if wallet_path.host_str().is_none() {
            return Err(RemoteWalletError::InvalidDerivationPath(
                "missing remote wallet type".to_string(),
            ));
        }

        let mut wallet_info = RemoteWalletInfo::default();
        wallet_info.manufacturer = wallet_path.host_str().unwrap().to_string();

        if let Some(wallet_id) = wallet_path.path_segments().map(|c| c.collect::<Vec<_>>()) {
            if wallet_id[0] != "" {
                wallet_info.pubkey = Pubkey::from_str(wallet_id[0]).map_err(|e| {
                    RemoteWalletError::InvalidDerivationPath(format!(
                        "pubkey from_str error: {:?}",
                        e
                    ))
                })?;
            }
        }

        let mut derivation_path = DerivationPath::default();
        let mut query_pairs = wallet_path.query_pairs();
        if query_pairs.count() > 0 {
            for _ in 0..query_pairs.count() {
                if let Some(mut pair) = query_pairs.next() {
                    if pair.0 == "key" {
                        let key_path = pair.1.to_mut();
                        let _key_path = key_path.clone();
                        if key_path.ends_with('/') {
                            key_path.pop();
                        }
                        let mut parts = key_path.split('/');
                        derivation_path.account = parts
                            .next()
                            .and_then(|account| account.replace("'", "").parse::<u32>().ok());
                        derivation_path.change = parts
                            .next()
                            .and_then(|change| change.replace("'", "").parse::<u32>().ok());
                        if parts.next().is_some() {
                            return Err(RemoteWalletError::InvalidDerivationPath(format!(
                                "key path `{}` too deep, only <account>/<change> supported",
                                _key_path
                            )));
                        }
                    } else {
                        return Err(RemoteWalletError::InvalidDerivationPath(format!(
                            "invalid query string `{}={}`, only `key` supported",
                            pair.0, pair.1
                        )));
                    }
                }
                if query_pairs.next().is_some() {
                    return Err(RemoteWalletError::InvalidDerivationPath(
                        "invalid query string, extra fields not supported".to_string(),
                    ));
                }
            }
        }
        Ok((wallet_info, derivation_path))
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

#[derive(Default, PartialEq, Clone)]
pub struct DerivationPath {
    pub account: Option<u32>,
    pub change: Option<u32>,
}

impl fmt::Debug for DerivationPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let account = if let Some(account) = self.account {
            format!("/{:?}'", account)
        } else {
            "".to_string()
        };
        let change = if let Some(change) = self.change {
            format!("/{:?}'", change)
        } else {
            "".to_string()
        };
        write!(f, "m/44'/501'{}{}", account, change)
    }
}

impl DerivationPath {
    pub fn get_query(&self) -> String {
        if let Some(account) = self.account {
            if let Some(change) = self.change {
                format!("?key={}/{}", account, change)
            } else {
                format!("?key={}", account)
            }
        } else {
            "".to_string()
        }
    }
}

/// Helper to determine if a device is a valid HID
pub fn is_valid_hid_device(usage_page: u16, interface_number: i32) -> bool {
    usage_page == HID_GLOBAL_USAGE_PAGE || interface_number == HID_USB_DEVICE_CLASS as i32
}

/// Helper to initialize hidapi and RemoteWalletManager
pub fn initialize_wallet_manager() -> Result<Arc<RemoteWalletManager>, RemoteWalletError> {
    let hidapi = Arc::new(Mutex::new(hidapi::HidApi::new()?));
    Ok(RemoteWalletManager::new(hidapi))
}

pub fn maybe_wallet_manager() -> Result<Option<Arc<RemoteWalletManager>>, RemoteWalletError> {
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
    fn test_parse_path() {
        let pubkey = Pubkey::new_rand();
        let (wallet_info, derivation_path) =
            RemoteWalletInfo::parse_path(format!("usb://ledger/{:?}?key=1/2", pubkey)).unwrap();
        assert!(wallet_info.matches(&RemoteWalletInfo {
            model: "nano-s".to_string(),
            manufacturer: "ledger".to_string(),
            serial: "".to_string(),
            pubkey,
            error: None,
        }));
        assert_eq!(
            derivation_path,
            DerivationPath {
                account: Some(1),
                change: Some(2),
            }
        );
        let (wallet_info, derivation_path) =
            RemoteWalletInfo::parse_path(format!("usb://ledger/{:?}?key=1'/2'", pubkey)).unwrap();
        assert!(wallet_info.matches(&RemoteWalletInfo {
            model: "nano-s".to_string(),
            manufacturer: "ledger".to_string(),
            serial: "".to_string(),
            pubkey,
            error: None,
        }));
        assert_eq!(
            derivation_path,
            DerivationPath {
                account: Some(1),
                change: Some(2),
            }
        );
        let (wallet_info, derivation_path) =
            RemoteWalletInfo::parse_path(format!("usb://ledger/{:?}?key=1\'/2\'", pubkey)).unwrap();
        assert!(wallet_info.matches(&RemoteWalletInfo {
            model: "nano-s".to_string(),
            manufacturer: "ledger".to_string(),
            serial: "".to_string(),
            pubkey,
            error: None,
        }));
        assert_eq!(
            derivation_path,
            DerivationPath {
                account: Some(1),
                change: Some(2),
            }
        );
        let (wallet_info, derivation_path) =
            RemoteWalletInfo::parse_path(format!("usb://ledger/{:?}?key=1/2/", pubkey)).unwrap();
        assert!(wallet_info.matches(&RemoteWalletInfo {
            model: "nano-s".to_string(),
            manufacturer: "ledger".to_string(),
            serial: "".to_string(),
            pubkey,
            error: None,
        }));
        assert_eq!(
            derivation_path,
            DerivationPath {
                account: Some(1),
                change: Some(2),
            }
        );
        let (wallet_info, derivation_path) =
            RemoteWalletInfo::parse_path(format!("usb://ledger/{:?}?key=1/", pubkey)).unwrap();
        assert!(wallet_info.matches(&RemoteWalletInfo {
            model: "nano-s".to_string(),
            manufacturer: "ledger".to_string(),
            serial: "".to_string(),
            pubkey,
            error: None,
        }));
        assert_eq!(
            derivation_path,
            DerivationPath {
                account: Some(1),
                change: None,
            }
        );

        // Test that wallet id need not be complete for key derivation to work
        let (wallet_info, derivation_path) =
            RemoteWalletInfo::parse_path("usb://ledger?key=1".to_string()).unwrap();
        assert!(wallet_info.matches(&RemoteWalletInfo {
            model: "nano-s".to_string(),
            manufacturer: "ledger".to_string(),
            serial: "".to_string(),
            pubkey: Pubkey::default(),
            error: None,
        }));
        assert_eq!(
            derivation_path,
            DerivationPath {
                account: Some(1),
                change: None,
            }
        );
        let (wallet_info, derivation_path) =
            RemoteWalletInfo::parse_path("usb://ledger/?key=1/2".to_string()).unwrap();
        assert!(wallet_info.matches(&RemoteWalletInfo {
            model: "".to_string(),
            manufacturer: "ledger".to_string(),
            serial: "".to_string(),
            pubkey: Pubkey::default(),
            error: None,
        }));
        assert_eq!(
            derivation_path,
            DerivationPath {
                account: Some(1),
                change: Some(2),
            }
        );

        // Failure cases
        assert!(
            RemoteWalletInfo::parse_path("usb://ledger/bad-pubkey?key=1/2".to_string()).is_err()
        );
        assert!(RemoteWalletInfo::parse_path("usb://?key=1/2".to_string()).is_err());
        assert!(RemoteWalletInfo::parse_path("usb:/ledger?key=1/2".to_string()).is_err());
        assert!(RemoteWalletInfo::parse_path("ledger?key=1/2".to_string()).is_err());
        assert!(RemoteWalletInfo::parse_path("usb://ledger?key=1/2/3".to_string()).is_err());
        // Other query strings cause an error
        assert!(
            RemoteWalletInfo::parse_path("usb://ledger/?key=1/2&test=other".to_string()).is_err()
        );
        assert!(RemoteWalletInfo::parse_path("usb://ledger/?Key=1/2".to_string()).is_err());
        assert!(RemoteWalletInfo::parse_path("usb://ledger/?test=other".to_string()).is_err());
    }

    #[test]
    fn test_remote_wallet_info_matches() {
        let pubkey = Pubkey::new_rand();
        let info = RemoteWalletInfo {
            manufacturer: "Ledger".to_string(),
            model: "Nano S".to_string(),
            serial: "0001".to_string(),
            pubkey: pubkey.clone(),
            error: None,
        };
        let mut test_info = RemoteWalletInfo::default();
        test_info.manufacturer = "Not Ledger".to_string();
        assert!(!info.matches(&test_info));
        test_info.manufacturer = "Ledger".to_string();
        assert!(info.matches(&test_info));
        test_info.model = "Other".to_string();
        assert!(info.matches(&test_info));
        test_info.model = "Nano S".to_string();
        assert!(info.matches(&test_info));
        let another_pubkey = Pubkey::new_rand();
        test_info.pubkey = another_pubkey;
        assert!(!info.matches(&test_info));
        test_info.pubkey = pubkey;
        assert!(info.matches(&test_info));
    }

    #[test]
    fn test_get_pretty_path() {
        let pubkey = Pubkey::new_rand();
        let pubkey_str = pubkey.to_string();
        let remote_wallet_info = RemoteWalletInfo {
            model: "nano-s".to_string(),
            manufacturer: "ledger".to_string(),
            serial: "".to_string(),
            pubkey,
            error: None,
        };
        assert_eq!(
            remote_wallet_info.get_pretty_path(),
            format!("usb://ledger/{}", pubkey_str)
        );
    }

    #[test]
    fn test_get_query() {
        let derivation_path = DerivationPath {
            account: None,
            change: None,
        };
        assert_eq!(derivation_path.get_query(), "".to_string());
        let derivation_path = DerivationPath {
            account: Some(1),
            change: None,
        };
        assert_eq!(derivation_path.get_query(), "?key=1".to_string());
        let derivation_path = DerivationPath {
            account: Some(1),
            change: Some(2),
        };
        assert_eq!(derivation_path.get_query(), "?key=1/2".to_string());
    }
}
