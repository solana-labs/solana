use {
    core::{iter::IntoIterator, slice::Iter},
    derivation_path::{ChildIndex, DerivationPath as DerivationPathInner},
    std::{
        convert::{Infallible, TryFrom},
        fmt,
        str::FromStr,
    },
    thiserror::Error,
    uriparse::URIReference,
};

const ACCOUNT_INDEX: usize = 2;
const CHANGE_INDEX: usize = 3;

/// Derivation path error.
#[derive(Error, Debug, Clone, PartialEq)]
pub enum DerivationPathError {
    #[error("invalid derivation path: {0}")]
    InvalidDerivationPath(String),
    #[error("infallible")]
    Infallible,
}

impl From<Infallible> for DerivationPathError {
    fn from(_: Infallible) -> Self {
        Self::Infallible
    }
}

#[derive(Clone, PartialEq)]
pub struct DerivationPath(DerivationPathInner);

impl Default for DerivationPath {
    fn default() -> Self {
        Self::new_bip44(None, None)
    }
}

impl TryFrom<&str> for DerivationPath {
    type Error = DerivationPathError;
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Self::from_key_str(s)
    }
}

impl DerivationPath {
    fn new<P: Into<Box<[ChildIndex]>>>(path: P) -> Self {
        Self(DerivationPathInner::new(path))
    }

    pub fn from_key_str(path: &str) -> Result<Self, DerivationPathError> {
        Self::from_key_str_with_coin(path, Solana)
    }

    fn from_key_str_with_coin<T: Bip44>(path: &str, coin: T) -> Result<Self, DerivationPathError> {
        let path = format!("m/{}", path);
        let extend = DerivationPathInner::from_str(&path)
            .map_err(|err| DerivationPathError::InvalidDerivationPath(err.to_string()))?;
        let mut extend = extend.into_iter();
        let account = extend.next().map(|index| index.to_u32());
        let change = extend.next().map(|index| index.to_u32());
        if extend.next().is_some() {
            return Err(DerivationPathError::InvalidDerivationPath(format!(
                "key path `{}` too deep, only <account>/<change> supported",
                path
            )));
        }
        Ok(Self::new_bip44_with_coin(coin, account, change))
    }

    fn _from_absolute_path_str(path: &str) -> Result<Self, DerivationPathError> {
        let inner = DerivationPath::_from_absolute_path_insecure_str(path)?
            .into_iter()
            .map(|c| ChildIndex::Hardened(c.to_u32()))
            .collect::<Vec<_>>();
        Ok(Self(DerivationPathInner::new(inner)))
    }

    fn _from_absolute_path_insecure_str(path: &str) -> Result<Self, DerivationPathError> {
        Ok(Self(DerivationPathInner::from_str(&path).map_err(
            |err| DerivationPathError::InvalidDerivationPath(err.to_string()),
        )?))
    }

    pub fn new_bip44(account: Option<u32>, change: Option<u32>) -> Self {
        Self::new_bip44_with_coin(Solana, account, change)
    }

    fn new_bip44_with_coin<T: Bip44>(coin: T, account: Option<u32>, change: Option<u32>) -> Self {
        let mut indexes = coin.base_indexes();
        if let Some(account) = account {
            indexes.push(ChildIndex::Hardened(account));
            if let Some(change) = change {
                indexes.push(ChildIndex::Hardened(change));
            }
        }
        Self::new(indexes)
    }

    pub fn account(&self) -> Option<&ChildIndex> {
        self.0.path().get(ACCOUNT_INDEX)
    }

    pub fn change(&self) -> Option<&ChildIndex> {
        self.0.path().get(CHANGE_INDEX)
    }

    pub fn path(&self) -> &[ChildIndex] {
        self.0.path()
    }

    // Assumes `key` query-string key
    pub fn get_query(&self) -> String {
        if let Some(account) = &self.account() {
            if let Some(change) = &self.change() {
                format!("?key={}/{}", account, change)
            } else {
                format!("?key={}", account)
            }
        } else {
            "".to_string()
        }
    }

    // Only accepts single query string pair of type `key`
    pub fn from_uri(uri: &URIReference<'_>) -> Result<Option<Self>, DerivationPathError> {
        if let Some(query) = uri.query() {
            let query_str = query.as_str();
            if query_str.is_empty() {
                return Ok(None);
            }
            let query = qstring::QString::from(query_str);
            if query.len() > 1 {
                return Err(DerivationPathError::InvalidDerivationPath(
                    "invalid query string, extra fields not supported".to_string(),
                ));
            }
            let key = query.get("key");
            if key.is_none() {
                return Err(DerivationPathError::InvalidDerivationPath(format!(
                    "invalid query string `{}`, only `key` supported",
                    query_str,
                )));
            }
            // Use from_key_str instead of TryInto here to make it a little more explicit that this
            // generates a Solana bip44 DerivationPath
            key.map(Self::from_key_str).transpose()
        } else {
            Ok(None)
        }
    }
}

impl fmt::Debug for DerivationPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "m")?;
        for index in self.0.path() {
            write!(f, "/{}", index)?;
        }
        Ok(())
    }
}

impl<'a> IntoIterator for &'a DerivationPath {
    type IntoIter = Iter<'a, ChildIndex>;
    type Item = &'a ChildIndex;
    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

trait Bip44 {
    const PURPOSE: u32 = 44;
    const COIN: u32;

    fn base_indexes(&self) -> Vec<ChildIndex> {
        vec![
            ChildIndex::Hardened(Self::PURPOSE),
            ChildIndex::Hardened(Self::COIN),
        ]
    }
}

struct Solana;

impl Bip44 for Solana {
    const COIN: u32 = 501;
}

#[cfg(test)]
mod tests {
    use super::*;
    use uriparse::URIReferenceBuilder;

    struct TestCoin;
    impl Bip44 for TestCoin {
        const COIN: u32 = 999;
    }

    #[test]
    fn test_from_key_str() {
        let s = "1/2";
        assert_eq!(
            DerivationPath::from_key_str_with_coin(s, TestCoin).unwrap(),
            DerivationPath::new_bip44_with_coin(TestCoin, Some(1), Some(2))
        );
        let s = "1'/2'";
        assert_eq!(
            DerivationPath::from_key_str_with_coin(s, TestCoin).unwrap(),
            DerivationPath::new_bip44_with_coin(TestCoin, Some(1), Some(2))
        );
        let s = "1\'/2\'";
        assert_eq!(
            DerivationPath::from_key_str_with_coin(s, TestCoin).unwrap(),
            DerivationPath::new_bip44_with_coin(TestCoin, Some(1), Some(2))
        );
        let s = "1";
        assert_eq!(
            DerivationPath::from_key_str_with_coin(s, TestCoin).unwrap(),
            DerivationPath::new_bip44_with_coin(TestCoin, Some(1), None)
        );
        let s = "1'";
        assert_eq!(
            DerivationPath::from_key_str_with_coin(s, TestCoin).unwrap(),
            DerivationPath::new_bip44_with_coin(TestCoin, Some(1), None)
        );
        let s = "1\'";
        assert_eq!(
            DerivationPath::from_key_str_with_coin(s, TestCoin).unwrap(),
            DerivationPath::new_bip44_with_coin(TestCoin, Some(1), None)
        );

        assert!(DerivationPath::from_key_str_with_coin("1/2/3", TestCoin).is_err());
        assert!(DerivationPath::from_key_str_with_coin("other", TestCoin).is_err());
        assert!(DerivationPath::from_key_str_with_coin("1o", TestCoin).is_err());
    }

    #[test]
    fn test_from_absolute_path_str() {
        let s = "m/44/501";
        assert_eq!(
            DerivationPath::_from_absolute_path_str(s).unwrap(),
            DerivationPath::default()
        );
        let s = "m/44'/501'";
        assert_eq!(
            DerivationPath::_from_absolute_path_str(s).unwrap(),
            DerivationPath::default()
        );
        let s = "m/44'/501'/1/2";
        assert_eq!(
            DerivationPath::_from_absolute_path_str(s).unwrap(),
            DerivationPath::new_bip44(Some(1), Some(2))
        );
        let s = "m/44'/501'/1'/2'";
        assert_eq!(
            DerivationPath::_from_absolute_path_str(s).unwrap(),
            DerivationPath::new_bip44(Some(1), Some(2))
        );

        // Test non-Solana Bip44
        let s = "m/44'/999'/1/2";
        assert_eq!(
            DerivationPath::_from_absolute_path_str(s).unwrap(),
            DerivationPath::new_bip44_with_coin(TestCoin, Some(1), Some(2))
        );
        let s = "m/44'/999'/1'/2'";
        assert_eq!(
            DerivationPath::_from_absolute_path_str(s).unwrap(),
            DerivationPath::new_bip44_with_coin(TestCoin, Some(1), Some(2))
        );

        // Test non-bip44 paths
        let s = "m/501'/0'/0/0";
        assert_eq!(
            DerivationPath::_from_absolute_path_str(s).unwrap(),
            DerivationPath::new(vec![
                ChildIndex::Hardened(501),
                ChildIndex::Hardened(0),
                ChildIndex::Hardened(0),
                ChildIndex::Hardened(0),
            ])
        );
        let s = "m/501'/0'/0'/0'";
        assert_eq!(
            DerivationPath::_from_absolute_path_str(s).unwrap(),
            DerivationPath::new(vec![
                ChildIndex::Hardened(501),
                ChildIndex::Hardened(0),
                ChildIndex::Hardened(0),
                ChildIndex::Hardened(0),
            ])
        );
    }

    #[test]
    fn test_new_from_uri() {
        let derivation_path = DerivationPath::new_bip44(Some(0), Some(0));

        // test://path?key=0/0
        let mut builder = URIReferenceBuilder::new();
        builder
            .try_scheme(Some("test"))
            .unwrap()
            .try_authority(Some("path"))
            .unwrap()
            .try_path("")
            .unwrap()
            .try_query(Some("key=0/0"))
            .unwrap();
        let uri = builder.build().unwrap();
        assert_eq!(
            DerivationPath::from_uri(&uri).unwrap(),
            Some(derivation_path.clone())
        );

        // test://path?key=0'/0'
        let mut builder = URIReferenceBuilder::new();
        builder
            .try_scheme(Some("test"))
            .unwrap()
            .try_authority(Some("path"))
            .unwrap()
            .try_path("")
            .unwrap()
            .try_query(Some("key=0'/0'"))
            .unwrap();
        let uri = builder.build().unwrap();
        assert_eq!(
            DerivationPath::from_uri(&uri).unwrap(),
            Some(derivation_path.clone())
        );

        // test://path?key=0\'/0\'
        let mut builder = URIReferenceBuilder::new();
        builder
            .try_scheme(Some("test"))
            .unwrap()
            .try_authority(Some("path"))
            .unwrap()
            .try_path("")
            .unwrap()
            .try_query(Some("key=0\'/0\'"))
            .unwrap();
        let uri = builder.build().unwrap();
        assert_eq!(
            DerivationPath::from_uri(&uri).unwrap(),
            Some(derivation_path)
        );

        // test://path
        let mut builder = URIReferenceBuilder::new();
        builder
            .try_scheme(Some("test"))
            .unwrap()
            .try_authority(Some("path"))
            .unwrap()
            .try_path("")
            .unwrap();
        let uri = builder.build().unwrap();
        assert_eq!(DerivationPath::from_uri(&uri).unwrap(), None);

        // test://path?
        let mut builder = URIReferenceBuilder::new();
        builder
            .try_scheme(Some("test"))
            .unwrap()
            .try_authority(Some("path"))
            .unwrap()
            .try_path("")
            .unwrap()
            .try_query(Some(""))
            .unwrap();
        let uri = builder.build().unwrap();
        assert_eq!(DerivationPath::from_uri(&uri).unwrap(), None);

        // test://path?key=0/0/0
        let mut builder = URIReferenceBuilder::new();
        builder
            .try_scheme(Some("test"))
            .unwrap()
            .try_authority(Some("path"))
            .unwrap()
            .try_path("")
            .unwrap()
            .try_query(Some("key=0/0/0"))
            .unwrap();
        let uri = builder.build().unwrap();
        assert!(matches!(
            DerivationPath::from_uri(&uri),
            Err(DerivationPathError::InvalidDerivationPath(_))
        ));

        // test://path?key=0/0&bad-key=0/0
        let mut builder = URIReferenceBuilder::new();
        builder
            .try_scheme(Some("test"))
            .unwrap()
            .try_authority(Some("path"))
            .unwrap()
            .try_path("")
            .unwrap()
            .try_query(Some("key=0/0&bad-key=0/0"))
            .unwrap();
        let uri = builder.build().unwrap();
        assert!(matches!(
            DerivationPath::from_uri(&uri),
            Err(DerivationPathError::InvalidDerivationPath(_))
        ));

        // test://path?bad-key=0/0
        let mut builder = URIReferenceBuilder::new();
        builder
            .try_scheme(Some("test"))
            .unwrap()
            .try_authority(Some("path"))
            .unwrap()
            .try_path("")
            .unwrap()
            .try_query(Some("bad-key=0/0"))
            .unwrap();
        let uri = builder.build().unwrap();
        assert!(matches!(
            DerivationPath::from_uri(&uri),
            Err(DerivationPathError::InvalidDerivationPath(_))
        ));

        // test://path?key=bad-value
        let mut builder = URIReferenceBuilder::new();
        builder
            .try_scheme(Some("test"))
            .unwrap()
            .try_authority(Some("path"))
            .unwrap()
            .try_path("")
            .unwrap()
            .try_query(Some("key=bad-value"))
            .unwrap();
        let uri = builder.build().unwrap();
        assert!(matches!(
            DerivationPath::from_uri(&uri),
            Err(DerivationPathError::InvalidDerivationPath(_))
        ));

        // test://path?key=
        let mut builder = URIReferenceBuilder::new();
        builder
            .try_scheme(Some("test"))
            .unwrap()
            .try_authority(Some("path"))
            .unwrap()
            .try_path("")
            .unwrap()
            .try_query(Some("key="))
            .unwrap();
        let uri = builder.build().unwrap();
        assert!(matches!(
            DerivationPath::from_uri(&uri),
            Err(DerivationPathError::InvalidDerivationPath(_))
        ));

        // test://path?key
        let mut builder = URIReferenceBuilder::new();
        builder
            .try_scheme(Some("test"))
            .unwrap()
            .try_authority(Some("path"))
            .unwrap()
            .try_path("")
            .unwrap()
            .try_query(Some("key"))
            .unwrap();
        let uri = builder.build().unwrap();
        assert!(matches!(
            DerivationPath::from_uri(&uri),
            Err(DerivationPathError::InvalidDerivationPath(_))
        ));
    }

    #[test]
    fn test_get_query() {
        let derivation_path = DerivationPath::new_bip44_with_coin(TestCoin, None, None);
        assert_eq!(derivation_path.get_query(), "".to_string());
        let derivation_path = DerivationPath::new_bip44_with_coin(TestCoin, Some(1), None);
        assert_eq!(derivation_path.get_query(), "?key=1'".to_string());
        let derivation_path = DerivationPath::new_bip44_with_coin(TestCoin, Some(1), Some(2));
        assert_eq!(derivation_path.get_query(), "?key=1'/2'".to_string());
    }

    #[test]
    fn test_derivation_path_debug() {
        let path = DerivationPath::default();
        assert_eq!(format!("{:?}", path), "m/44'/501'".to_string());

        let path = DerivationPath::new_bip44(Some(1), None);
        assert_eq!(format!("{:?}", path), "m/44'/501'/1'".to_string());

        let path = DerivationPath::new_bip44(Some(1), Some(2));
        assert_eq!(format!("{:?}", path), "m/44'/501'/1'/2'".to_string());
    }
}
