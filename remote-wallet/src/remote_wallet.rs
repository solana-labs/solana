use {
    crate::{
        ledger::{is_valid_ledger, LedgerWallet},
        ledger_error::LedgerError,
    },
    log::*,
    parking_lot::{Mutex, RwLock},
    solana_sdk::{
        derivation_path::{DerivationPath, DerivationPathError},
        pubkey::{ParsePubkeyError, Pubkey},
        signature::{Signature, SignerError},
    },
    std::{
        convert::{Infallible, TryFrom, TryInto},
        str::FromStr,
        sync::Arc,
        time::{Duration, Instant},
    },
    thiserror::Error,
    uriparse::{URIReference, URIReferenceBuilder, URIReferenceError},
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

        let mut detected_devices = vec![];
        let mut errors = vec![];
        for device_info in devices.filter(|&device_info| {
            is_valid_hid_device(device_info.usage_page(), device_info.interface_number())
                && is_valid_ledger(device_info.vendor_id(), device_info.product_id())
        }) {
            match usb.open_path(&device_info.path()) {
                Ok(device) => {
                    let mut ledger = LedgerWallet::new(device);
                    let result = ledger.read_device(&device_info);
                    match result {
                        Ok(info) => {
                            ledger.pretty_path = info.get_pretty_path();
                            let path = device_info.path().to_str().unwrap().to_string();
                            trace!("Found device: {:?}", info);
                            detected_devices.push(Device {
                                path,
                                info,
                                wallet_type: RemoteWalletType::Ledger(Arc::new(ledger)),
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

    /// List connected and acknowledged wallets
    pub fn list_devices(&self) -> Vec<RemoteWalletInfo> {
        self.devices.read().iter().map(|d| d.info.clone()).collect()
    }

    /// Get a particular wallet
    #[allow(unreachable_patterns)]
    pub fn get_ledger(
        &self,
        host_device_path: &str,
    ) -> Result<Arc<LedgerWallet>, RemoteWalletError> {
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

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Manufacturer {
    Unknown,
    Ledger,
}

impl Default for Manufacturer {
    fn default() -> Self {
        Self::Unknown
    }
}

const MFR_UNKNOWN: &str = "unknown";
const MFR_LEDGER: &str = "ledger";

#[derive(Clone, Debug, Error, PartialEq)]
#[error("not a manufacturer")]
pub struct ManufacturerError;

impl From<Infallible> for ManufacturerError {
    fn from(_: Infallible) -> Self {
        ManufacturerError
    }
}

impl FromStr for Manufacturer {
    type Err = ManufacturerError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.to_ascii_lowercase();
        match s.as_str() {
            MFR_LEDGER => Ok(Self::Ledger),
            _ => Err(ManufacturerError),
        }
    }
}

impl TryFrom<&str> for Manufacturer {
    type Error = ManufacturerError;
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Manufacturer::from_str(s)
    }
}

impl AsRef<str> for Manufacturer {
    fn as_ref(&self) -> &str {
        match self {
            Self::Unknown => MFR_UNKNOWN,
            Self::Ledger => MFR_LEDGER,
        }
    }
}

impl std::fmt::Display for Manufacturer {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let s: &str = self.as_ref();
        write!(f, "{}", s)
    }
}

#[derive(Clone, Debug, Error, PartialEq)]
pub enum LocatorError {
    #[error(transparent)]
    ManufacturerError(#[from] ManufacturerError),
    #[error(transparent)]
    PubkeyError(#[from] ParsePubkeyError),
    #[error(transparent)]
    DerivationPathError(#[from] DerivationPathError),
    #[error(transparent)]
    UriReferenceError(#[from] URIReferenceError),
    #[error("unimplemented scheme")]
    UnimplementedScheme,
    #[error("infallible")]
    Infallible,
}

impl From<Infallible> for LocatorError {
    fn from(_: Infallible) -> Self {
        Self::Infallible
    }
}

#[derive(Debug, PartialEq)]
pub struct Locator {
    manufacturer: Manufacturer,
    pubkey: Option<Pubkey>,
    derivation_path: Option<DerivationPath>,
}

impl std::fmt::Display for Locator {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let maybe_path = self.pubkey.map(|p| p.to_string());
        let path = maybe_path.as_deref().unwrap_or("/");
        let maybe_query = self.derivation_path.as_ref().map(|d| d.get_query());
        let maybe_query2 = maybe_query.as_ref().map(|q| &q[1..]);

        let mut builder = URIReferenceBuilder::new();
        builder
            .try_scheme(Some("usb"))
            .unwrap()
            .try_authority(Some(self.manufacturer.as_ref()))
            .unwrap()
            .try_path(path)
            .unwrap()
            .try_query(maybe_query2)
            .unwrap();

        let uri = builder.build().unwrap();
        write!(f, "{}", uri)
    }
}

impl Locator {
    pub fn new_from_path<P: AsRef<str>>(path: P) -> Result<Self, LocatorError> {
        let path = path.as_ref();
        let uri = URIReference::try_from(path)?;
        Self::new_from_uri(&uri)
    }

    pub fn new_from_uri(uri: &URIReference<'_>) -> Result<Self, LocatorError> {
        let scheme = uri.scheme().map(|s| s.as_str().to_ascii_lowercase());
        let host = uri.host().map(|h| h.to_string());
        match (scheme, host) {
            (Some(scheme), Some(host)) if scheme == "usb" => {
                let path = uri.path().segments().get(0).and_then(|s| {
                    if !s.is_empty() {
                        Some(s.as_str())
                    } else {
                        None
                    }
                });
                let key = if let Some(query) = uri.query() {
                    let query_str = query.as_str();
                    let query = qstring::QString::from(query_str);
                    if query.len() > 1 {
                        return Err(DerivationPathError::InvalidDerivationPath(
                            "invalid query string, extra fields not supported".to_string(),
                        )
                        .into());
                    }
                    let key = query.get("key");
                    if key.is_none() {
                        return Err(DerivationPathError::InvalidDerivationPath(format!(
                            "invalid query string `{}`, only `key` supported",
                            query_str,
                        ))
                        .into());
                    }
                    key.map(|v| v.to_string())
                } else {
                    None
                };
                Self::new_from_parts(host.as_str(), path, key.as_deref())
            }
            (Some(_scheme), Some(_host)) => Err(LocatorError::UnimplementedScheme),
            (None, Some(_host)) => Err(LocatorError::UnimplementedScheme),
            (_, None) => Err(LocatorError::ManufacturerError(ManufacturerError)),
        }
    }

    pub fn new_from_parts<V, VE, P, PE, D, DE>(
        manufacturer: V,
        pubkey: Option<P>,
        derivation_path: Option<D>,
    ) -> Result<Self, LocatorError>
    where
        VE: Into<LocatorError>,
        V: TryInto<Manufacturer, Error = VE>,
        PE: Into<LocatorError>,
        P: TryInto<Pubkey, Error = PE>,
        DE: Into<LocatorError>,
        D: TryInto<DerivationPath, Error = DE>,
    {
        let manufacturer = manufacturer.try_into().map_err(|e| e.into())?;
        let pubkey = if let Some(pubkeyable) = pubkey {
            Some(pubkeyable.try_into().map_err(|e| e.into())?)
        } else {
            None
        };
        let derivation_path = if let Some(derivation_path) = derivation_path {
            Some(derivation_path.try_into().map_err(|e| e.into())?)
        } else {
            None
        };
        Ok(Self {
            manufacturer,
            pubkey,
            derivation_path,
        })
    }
}

impl RemoteWalletInfo {
    pub fn parse_path(path: String) -> Result<(Self, DerivationPath), RemoteWalletError> {
        let Locator {
            manufacturer,
            pubkey,
            derivation_path,
        } = Locator::new_from_path(path)?;
        Ok((
            RemoteWalletInfo {
                manufacturer,
                pubkey: pubkey.unwrap_or_default(),
                ..RemoteWalletInfo::default()
            },
            derivation_path.unwrap_or_default(),
        ))
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
        let pubkey = solana_sdk::pubkey::new_rand();
        let (wallet_info, derivation_path) =
            RemoteWalletInfo::parse_path(format!("usb://ledger/{:?}?key=1/2", pubkey)).unwrap();
        assert!(wallet_info.matches(&RemoteWalletInfo {
            model: "nano-s".to_string(),
            manufacturer: Manufacturer::Ledger,
            serial: "".to_string(),
            host_device_path: "/host/device/path".to_string(),
            pubkey,
            error: None,
        }));
        assert_eq!(derivation_path, DerivationPath::new_bip44(Some(1), Some(2)));
        let (wallet_info, derivation_path) =
            RemoteWalletInfo::parse_path(format!("usb://ledger/{:?}?key=1'/2'", pubkey)).unwrap();
        assert!(wallet_info.matches(&RemoteWalletInfo {
            model: "nano-s".to_string(),
            manufacturer: Manufacturer::Ledger,
            serial: "".to_string(),
            host_device_path: "/host/device/path".to_string(),
            pubkey,
            error: None,
        }));
        assert_eq!(derivation_path, DerivationPath::new_bip44(Some(1), Some(2)));
        let (wallet_info, derivation_path) =
            RemoteWalletInfo::parse_path(format!("usb://ledger/{:?}?key=1\'/2\'", pubkey)).unwrap();
        assert!(wallet_info.matches(&RemoteWalletInfo {
            model: "nano-s".to_string(),
            manufacturer: Manufacturer::Ledger,
            serial: "".to_string(),
            host_device_path: "/host/device/path".to_string(),
            pubkey,
            error: None,
        }));
        assert_eq!(derivation_path, DerivationPath::new_bip44(Some(1), Some(2)));

        // Test that wallet id need not be complete for key derivation to work
        let (wallet_info, derivation_path) =
            RemoteWalletInfo::parse_path("usb://ledger?key=1".to_string()).unwrap();
        assert!(wallet_info.matches(&RemoteWalletInfo {
            model: "nano-s".to_string(),
            manufacturer: Manufacturer::Ledger,
            serial: "".to_string(),
            host_device_path: "/host/device/path".to_string(),
            pubkey: Pubkey::default(),
            error: None,
        }));
        assert_eq!(derivation_path, DerivationPath::new_bip44(Some(1), None));
        let (wallet_info, derivation_path) =
            RemoteWalletInfo::parse_path("usb://ledger/?key=1/2".to_string()).unwrap();
        assert!(wallet_info.matches(&RemoteWalletInfo {
            model: "".to_string(),
            manufacturer: Manufacturer::Ledger,
            serial: "".to_string(),
            host_device_path: "/host/device/path".to_string(),
            pubkey: Pubkey::default(),
            error: None,
        }));
        assert_eq!(derivation_path, DerivationPath::new_bip44(Some(1), Some(2)));

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
            format!("usb://ledger/{}", pubkey_str)
        );
    }

    #[test]
    fn test_manufacturer() {
        assert_eq!(MFR_LEDGER.try_into(), Ok(Manufacturer::Ledger));
        assert!(matches!(Manufacturer::from_str(MFR_LEDGER), Ok(v) if v == Manufacturer::Ledger));
        assert_eq!(Manufacturer::Ledger.as_ref(), MFR_LEDGER);

        assert!(
            matches!(Manufacturer::from_str("bad-manufacturer"), Err(e) if e == ManufacturerError)
        );
    }

    #[test]
    fn test_locator_new_from_parts() {
        let manufacturer = Manufacturer::Ledger;
        let manufacturer_str = "ledger";
        let pubkey = Pubkey::new_unique();
        let pubkey_str = pubkey.to_string();
        let derivation_path = DerivationPath::new_bip44(Some(0), Some(0));
        let derivation_path_str = "0/0";

        let expect = Locator {
            manufacturer,
            pubkey: None,
            derivation_path: None,
        };
        assert!(matches!(
            Locator::new_from_parts(manufacturer, None::<Pubkey>, None::<DerivationPath>),
            Ok(e) if e == expect,
        ));
        assert!(matches!(
            Locator::new_from_parts(manufacturer_str, None::<Pubkey>, None::<DerivationPath>),
            Ok(e) if e == expect,
        ));

        let expect = Locator {
            manufacturer,
            pubkey: Some(pubkey),
            derivation_path: None,
        };
        assert!(matches!(
            Locator::new_from_parts(manufacturer, Some(pubkey), None::<DerivationPath>),
            Ok(e) if e == expect,
        ));
        assert!(matches!(
            Locator::new_from_parts(manufacturer_str, Some(pubkey_str.as_str()), None::<DerivationPath>),
            Ok(e) if e == expect,
        ));

        let expect = Locator {
            manufacturer,
            pubkey: None,
            derivation_path: Some(derivation_path.clone()),
        };
        assert!(matches!(
            Locator::new_from_parts(manufacturer, None::<Pubkey>, Some(derivation_path)),
            Ok(e) if e == expect,
        ));
        assert!(matches!(
            Locator::new_from_parts(manufacturer, None::<Pubkey>, Some(derivation_path_str)),
            Ok(e) if e == expect,
        ));

        assert!(matches!(
            Locator::new_from_parts("bad-manufacturer", None::<Pubkey>, None::<DerivationPath>),
            Err(LocatorError::ManufacturerError(e)) if e == ManufacturerError,
        ));
        assert!(matches!(
            Locator::new_from_parts(manufacturer, Some("bad-pubkey"), None::<DerivationPath>),
            Err(LocatorError::PubkeyError(e)) if e == ParsePubkeyError::Invalid,
        ));
        let bad_path = "bad-derivation-path".to_string();
        assert!(matches!(
            Locator::new_from_parts(manufacturer, None::<Pubkey>, Some(bad_path.as_str())),
            Err(LocatorError::DerivationPathError(
                DerivationPathError::InvalidDerivationPath(_)
            )),
        ));
    }

    #[test]
    fn test_locator_new_from_uri() {
        let derivation_path = DerivationPath::new_bip44(Some(0), Some(0));
        let manufacturer = Manufacturer::Ledger;
        let pubkey = Pubkey::new_unique();
        let pubkey_str = pubkey.to_string();

        // usb://ledger/{PUBKEY}?key=0'/0'
        let mut builder = URIReferenceBuilder::new();
        builder
            .try_scheme(Some("usb"))
            .unwrap()
            .try_authority(Some(Manufacturer::Ledger.as_ref()))
            .unwrap()
            .try_path(pubkey_str.as_str())
            .unwrap()
            .try_query(Some("key=0/0"))
            .unwrap();
        let uri = builder.build().unwrap();
        let expect = Locator {
            manufacturer,
            pubkey: Some(pubkey),
            derivation_path: Some(derivation_path.clone()),
        };
        assert_eq!(Locator::new_from_uri(&uri), Ok(expect));

        // usb://ledger/{PUBKEY}
        let mut builder = URIReferenceBuilder::new();
        builder
            .try_scheme(Some("usb"))
            .unwrap()
            .try_authority(Some(Manufacturer::Ledger.as_ref()))
            .unwrap()
            .try_path(pubkey_str.as_str())
            .unwrap();
        let uri = builder.build().unwrap();
        let expect = Locator {
            manufacturer,
            pubkey: Some(pubkey),
            derivation_path: None,
        };
        assert_eq!(Locator::new_from_uri(&uri), Ok(expect));

        // usb://ledger
        let mut builder = URIReferenceBuilder::new();
        builder
            .try_scheme(Some("usb"))
            .unwrap()
            .try_authority(Some(Manufacturer::Ledger.as_ref()))
            .unwrap()
            .try_path("")
            .unwrap();
        let uri = builder.build().unwrap();
        let expect = Locator {
            manufacturer,
            pubkey: None,
            derivation_path: None,
        };
        assert_eq!(Locator::new_from_uri(&uri), Ok(expect));

        // usb://ledger/
        let mut builder = URIReferenceBuilder::new();
        builder
            .try_scheme(Some("usb"))
            .unwrap()
            .try_authority(Some(Manufacturer::Ledger.as_ref()))
            .unwrap()
            .try_path("/")
            .unwrap();
        let uri = builder.build().unwrap();
        let expect = Locator {
            manufacturer,
            pubkey: None,
            derivation_path: None,
        };
        assert_eq!(Locator::new_from_uri(&uri), Ok(expect));

        // usb://ledger?key=0'/0'
        let mut builder = URIReferenceBuilder::new();
        builder
            .try_scheme(Some("usb"))
            .unwrap()
            .try_authority(Some(Manufacturer::Ledger.as_ref()))
            .unwrap()
            .try_path("")
            .unwrap()
            .try_query(Some("key=0/0"))
            .unwrap();
        let uri = builder.build().unwrap();
        let expect = Locator {
            manufacturer,
            pubkey: None,
            derivation_path: Some(derivation_path.clone()),
        };
        assert_eq!(Locator::new_from_uri(&uri), Ok(expect));

        // usb://ledger/?key=0'/0'
        let mut builder = URIReferenceBuilder::new();
        builder
            .try_scheme(Some("usb"))
            .unwrap()
            .try_authority(Some(Manufacturer::Ledger.as_ref()))
            .unwrap()
            .try_path("/")
            .unwrap()
            .try_query(Some("key=0/0"))
            .unwrap();
        let uri = builder.build().unwrap();
        let expect = Locator {
            manufacturer,
            pubkey: None,
            derivation_path: Some(derivation_path),
        };
        assert_eq!(Locator::new_from_uri(&uri), Ok(expect));

        // bad-scheme://ledger
        let mut builder = URIReferenceBuilder::new();
        builder
            .try_scheme(Some("bad-scheme"))
            .unwrap()
            .try_authority(Some(Manufacturer::Ledger.as_ref()))
            .unwrap()
            .try_path("")
            .unwrap();
        let uri = builder.build().unwrap();
        assert_eq!(
            Locator::new_from_uri(&uri),
            Err(LocatorError::UnimplementedScheme)
        );

        // usb://bad-manufacturer
        let mut builder = URIReferenceBuilder::new();
        builder
            .try_scheme(Some("usb"))
            .unwrap()
            .try_authority(Some("bad-manufacturer"))
            .unwrap()
            .try_path("")
            .unwrap();
        let uri = builder.build().unwrap();
        assert_eq!(
            Locator::new_from_uri(&uri),
            Err(LocatorError::ManufacturerError(ManufacturerError))
        );

        // usb://ledger/bad-pubkey
        let mut builder = URIReferenceBuilder::new();
        builder
            .try_scheme(Some("usb"))
            .unwrap()
            .try_authority(Some(Manufacturer::Ledger.as_ref()))
            .unwrap()
            .try_path("bad-pubkey")
            .unwrap();
        let uri = builder.build().unwrap();
        assert_eq!(
            Locator::new_from_uri(&uri),
            Err(LocatorError::PubkeyError(ParsePubkeyError::Invalid))
        );

        // usb://ledger?bad-key=0/0
        let mut builder = URIReferenceBuilder::new();
        builder
            .try_scheme(Some("usb"))
            .unwrap()
            .try_authority(Some(Manufacturer::Ledger.as_ref()))
            .unwrap()
            .try_path("")
            .unwrap()
            .try_query(Some("bad-key=0/0"))
            .unwrap();
        let uri = builder.build().unwrap();
        assert!(matches!(
            Locator::new_from_uri(&uri),
            Err(LocatorError::DerivationPathError(_))
        ));

        // usb://ledger?key=bad-value
        let mut builder = URIReferenceBuilder::new();
        builder
            .try_scheme(Some("usb"))
            .unwrap()
            .try_authority(Some(Manufacturer::Ledger.as_ref()))
            .unwrap()
            .try_path("")
            .unwrap()
            .try_query(Some("key=bad-value"))
            .unwrap();
        let uri = builder.build().unwrap();
        assert!(matches!(
            Locator::new_from_uri(&uri),
            Err(LocatorError::DerivationPathError(_))
        ));

        // usb://ledger?key=
        let mut builder = URIReferenceBuilder::new();
        builder
            .try_scheme(Some("usb"))
            .unwrap()
            .try_authority(Some(Manufacturer::Ledger.as_ref()))
            .unwrap()
            .try_path("")
            .unwrap()
            .try_query(Some("key="))
            .unwrap();
        let uri = builder.build().unwrap();
        assert!(matches!(
            Locator::new_from_uri(&uri),
            Err(LocatorError::DerivationPathError(_))
        ));

        // usb://ledger?key
        let mut builder = URIReferenceBuilder::new();
        builder
            .try_scheme(Some("usb"))
            .unwrap()
            .try_authority(Some(Manufacturer::Ledger.as_ref()))
            .unwrap()
            .try_path("")
            .unwrap()
            .try_query(Some("key"))
            .unwrap();
        let uri = builder.build().unwrap();
        assert!(matches!(
            Locator::new_from_uri(&uri),
            Err(LocatorError::DerivationPathError(_))
        ));
    }

    #[test]
    fn test_locator_new_from_path() {
        let derivation_path = DerivationPath::new_bip44(Some(0), Some(0));
        let manufacturer = Manufacturer::Ledger;
        let pubkey = Pubkey::new_unique();
        let path = format!("usb://ledger/{}?key=0/0", pubkey);
        Locator::new_from_path(path).unwrap();

        // usb://ledger/{PUBKEY}?key=0'/0'
        let path = format!("usb://ledger/{}?key=0'/0'", pubkey);
        let expect = Locator {
            manufacturer,
            pubkey: Some(pubkey),
            derivation_path: Some(derivation_path.clone()),
        };
        assert_eq!(Locator::new_from_path(path), Ok(expect));

        // usb://ledger/{PUBKEY}
        let path = format!("usb://ledger/{}", pubkey);
        let expect = Locator {
            manufacturer,
            pubkey: Some(pubkey),
            derivation_path: None,
        };
        assert_eq!(Locator::new_from_path(path), Ok(expect));

        // usb://ledger
        let path = "usb://ledger";
        let expect = Locator {
            manufacturer,
            pubkey: None,
            derivation_path: None,
        };
        assert_eq!(Locator::new_from_path(path), Ok(expect));

        // usb://ledger/
        let path = "usb://ledger/";
        let expect = Locator {
            manufacturer,
            pubkey: None,
            derivation_path: None,
        };
        assert_eq!(Locator::new_from_path(path), Ok(expect));

        // usb://ledger?key=0'/0'
        let path = "usb://ledger?key=0'/0'";
        let expect = Locator {
            manufacturer,
            pubkey: None,
            derivation_path: Some(derivation_path.clone()),
        };
        assert_eq!(Locator::new_from_path(path), Ok(expect));

        // usb://ledger/?key=0'/0'
        let path = "usb://ledger?key=0'/0'";
        let expect = Locator {
            manufacturer,
            pubkey: None,
            derivation_path: Some(derivation_path),
        };
        assert_eq!(Locator::new_from_path(path), Ok(expect));

        // bad-scheme://ledger
        let path = "bad-scheme://ledger";
        assert_eq!(
            Locator::new_from_path(path),
            Err(LocatorError::UnimplementedScheme)
        );

        // usb://bad-manufacturer
        let path = "usb://bad-manufacturer";
        assert_eq!(
            Locator::new_from_path(path),
            Err(LocatorError::ManufacturerError(ManufacturerError))
        );

        // usb://ledger/bad-pubkey
        let path = "usb://ledger/bad-pubkey";
        assert_eq!(
            Locator::new_from_path(path),
            Err(LocatorError::PubkeyError(ParsePubkeyError::Invalid))
        );

        // usb://ledger?bad-key=0/0
        let path = "usb://ledger?bad-key=0'/0'";
        assert!(matches!(
            Locator::new_from_path(path),
            Err(LocatorError::DerivationPathError(_))
        ));

        // usb://ledger?key=bad-value
        let path = format!("usb://ledger/{}?key=bad-value", pubkey);
        assert!(matches!(
            Locator::new_from_path(path),
            Err(LocatorError::DerivationPathError(_))
        ));

        // usb://ledger?key=
        let path = format!("usb://ledger/{}?key=", pubkey);
        assert!(matches!(
            Locator::new_from_path(path),
            Err(LocatorError::DerivationPathError(_))
        ));

        // usb://ledger?key
        let path = format!("usb://ledger/{}?key", pubkey);
        assert!(matches!(
            Locator::new_from_path(path),
            Err(LocatorError::DerivationPathError(_))
        ));
    }
}
