use {
    derivation_path::{ChildIndex, DerivationPath as DerivationPathInner},
    std::fmt,
    thiserror::Error,
};

const BIP_44_PURPOSE: u32 = 44; // not yet hardened
const BIP_44_COIN_SOL: u32 = 501; // not yet hardened
const ACCOUNT_INDEX: usize = 2;
const CHANGE_INDEX: usize = 3;

/// Derivation path error.
#[derive(Error, Debug, Clone)]
pub enum DerivationPathError {
    #[error("invalid derivation path: {0}")]
    InvalidDerivationPath(String),
}

#[derive(PartialEq)]
pub struct DerivationPath(DerivationPathInner);

impl Default for DerivationPath {
    fn default() -> Self {
        Self::new_bip44_solana(None, None)
    }
}

impl DerivationPath {
    fn new<P: Into<Box<[ChildIndex]>>>(path: P) -> Self {
        Self(DerivationPathInner::new(path))
    }

    pub fn from_key_str(path: &str) -> Result<Self, DerivationPathError> {
        let mut indexes = bip44_solana_indexes();
        let mut parts = path.split('/');
        if let Some(account) = parts.next() {
            indexes.push(child_index_from_str(account)?);
        }
        if let Some(change) = parts.next() {
            indexes.push(child_index_from_str(change)?);
        }
        if parts.next().is_some() {
            return Err(DerivationPathError::InvalidDerivationPath(format!(
                "key path `{}` too deep, only <account>/<change> supported",
                path
            )));
        }
        Ok(Self::new(indexes))
    }

    pub fn new_bip44_solana(account: Option<u32>, change: Option<u32>) -> Self {
        let mut indexes = bip44_solana_indexes();
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

fn bip44_solana_indexes() -> Vec<ChildIndex> {
    vec![
        ChildIndex::Hardened(BIP_44_PURPOSE),
        ChildIndex::Hardened(BIP_44_COIN_SOL),
    ]
}

fn child_index_from_str(s: &str) -> Result<ChildIndex, DerivationPathError> {
    let index_str = if let Some(stripped) = s.strip_suffix('\'') {
        stripped
    } else {
        s
    };
    let index = index_str.parse::<u32>().map_err(|_| {
        DerivationPathError::InvalidDerivationPath(format!(
            "failed to parse path component: {:?}",
            s
        ))
    })?;
    Ok(ChildIndex::Hardened(index))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_key_str() {
        let s = "1/2";
        assert_eq!(
            DerivationPath::from_key_str(s).unwrap(),
            DerivationPath::new_bip44_solana(Some(1), Some(2))
        );
        let s = "1'/2'";
        assert_eq!(
            DerivationPath::from_key_str(s).unwrap(),
            DerivationPath::new_bip44_solana(Some(1), Some(2))
        );
        let s = "1\'/2\'";
        assert_eq!(
            DerivationPath::from_key_str(s).unwrap(),
            DerivationPath::new_bip44_solana(Some(1), Some(2))
        );
        let s = "1";
        assert_eq!(
            DerivationPath::from_key_str(s).unwrap(),
            DerivationPath::new_bip44_solana(Some(1), None)
        );
        let s = "1'";
        assert_eq!(
            DerivationPath::from_key_str(s).unwrap(),
            DerivationPath::new_bip44_solana(Some(1), None)
        );
        let s = "1\'";
        assert_eq!(
            DerivationPath::from_key_str(s).unwrap(),
            DerivationPath::new_bip44_solana(Some(1), None)
        );

        assert!(DerivationPath::from_key_str("other").is_err());
        assert!(DerivationPath::from_key_str("1o").is_err());
    }

    #[test]
    fn test_get_query() {
        let derivation_path = DerivationPath::new_bip44_solana(None, None);
        assert_eq!(derivation_path.get_query(), "".to_string());
        let derivation_path = DerivationPath::new_bip44_solana(Some(1), None);
        assert_eq!(derivation_path.get_query(), "?key=1'".to_string());
        let derivation_path = DerivationPath::new_bip44_solana(Some(1), Some(2));
        assert_eq!(derivation_path.get_query(), "?key=1'/2'".to_string());
    }

    #[test]
    fn test_derivation_path_debug() {
        let path = DerivationPath::default();
        assert_eq!(format!("{:?}", path), "m/44'/501'".to_string());

        let path = DerivationPath::new_bip44_solana(Some(1), None);
        assert_eq!(format!("{:?}", path), "m/44'/501'/1'".to_string());

        let path = DerivationPath::new_bip44_solana(Some(1), Some(2));
        assert_eq!(format!("{:?}", path), "m/44'/501'/1'/2'".to_string());
    }

    #[test]
    fn test_child_index_from_str() {
        let s = "1";
        assert_eq!(child_index_from_str(s).unwrap(), ChildIndex::Hardened(1));
        let s = "1\'";
        assert_eq!(child_index_from_str(s).unwrap(), ChildIndex::Hardened(1));
        let s = "1/";
        assert!(child_index_from_str(s).is_err());
        let s = "e";
        assert!(child_index_from_str(s).is_err());
        let s = "m";
        assert!(child_index_from_str(s).is_err());
    }
}
