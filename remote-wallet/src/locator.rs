use {
    solana_sdk::{
        derivation_path::{DerivationPath, DerivationPathError},
        pubkey::{ParsePubkeyError, Pubkey},
    },
    std::{
        convert::{Infallible, TryFrom, TryInto},
        str::FromStr,
    },
    thiserror::Error,
    uriparse::{URIReference, URIReferenceBuilder, URIReferenceError},
};

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

const MANUFACTURER_UNKNOWN: &str = "unknown";
const MANUFACTURER_LEDGER: &str = "ledger";

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
            MANUFACTURER_LEDGER => Ok(Self::Ledger),
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
            Self::Unknown => MANUFACTURER_UNKNOWN,
            Self::Ledger => MANUFACTURER_LEDGER,
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
    pub manufacturer: Manufacturer,
    pub pubkey: Option<Pubkey>,
    pub derivation_path: Option<DerivationPath>,
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
        let pubkey = if let Some(pubkey) = pubkey {
            Some(pubkey.try_into().map_err(|e| e.into())?)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manufacturer() {
        assert_eq!(MANUFACTURER_LEDGER.try_into(), Ok(Manufacturer::Ledger));
        assert!(
            matches!(Manufacturer::from_str(MANUFACTURER_LEDGER), Ok(v) if v == Manufacturer::Ledger)
        );
        assert_eq!(Manufacturer::Ledger.as_ref(), MANUFACTURER_LEDGER);

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

        // usb://ledger?key=0/0
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

        // usb://ledger?key=0'/0'
        let mut builder = URIReferenceBuilder::new();
        builder
            .try_scheme(Some("usb"))
            .unwrap()
            .try_authority(Some(Manufacturer::Ledger.as_ref()))
            .unwrap()
            .try_path("")
            .unwrap()
            .try_query(Some("key=0'/0'"))
            .unwrap();
        let uri = builder.build().unwrap();
        let expect = Locator {
            manufacturer,
            pubkey: None,
            derivation_path: Some(derivation_path.clone()),
        };
        assert_eq!(Locator::new_from_uri(&uri), Ok(expect));

        // usb://ledger/?key=0/0
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
        let path = "usb://ledger?bad-key=0/0";
        assert!(matches!(
            Locator::new_from_path(path),
            Err(LocatorError::DerivationPathError(_))
        ));

        // usb://ledger?bad-key=0'/0'
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
