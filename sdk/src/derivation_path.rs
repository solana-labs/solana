use {
    std::{fmt, str::FromStr},
    thiserror::Error,
};

/// Derivation path error.
#[derive(Error, Debug, Clone)]
pub enum DerivationPathError {
    #[error("invalid derivation path: {0}")]
    InvalidDerivationPath(String),
}

#[derive(Clone, Default, PartialEq)]
pub struct DerivationPathComponent(u32);

impl DerivationPathComponent {
    pub const HARDENED_BIT: u32 = 1 << 31;

    pub fn as_u32(&self) -> u32 {
        self.0
    }
}

impl From<u32> for DerivationPathComponent {
    fn from(n: u32) -> Self {
        Self(n | Self::HARDENED_BIT)
    }
}

impl FromStr for DerivationPathComponent {
    type Err = DerivationPathError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let index_str = if let Some(stripped) = s.strip_suffix('\'') {
            stripped
        } else {
            s
        };
        index_str.parse::<u32>().map(|ki| ki.into()).map_err(|_| {
            DerivationPathError::InvalidDerivationPath(format!(
                "failed to parse path component: {:?}",
                s
            ))
        })
    }
}

impl std::fmt::Display for DerivationPathComponent {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        let hardened = if (self.0 & Self::HARDENED_BIT) == 0 {
            ""
        } else {
            "'"
        };
        let index = self.0 & !Self::HARDENED_BIT;
        write!(fmt, "{}{}", index, hardened)
    }
}

impl std::fmt::Debug for DerivationPathComponent {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        std::fmt::Display::fmt(self, fmt)
    }
}

#[derive(Default, PartialEq, Clone)]
pub struct DerivationPath {
    pub account: Option<DerivationPathComponent>,
    pub change: Option<DerivationPathComponent>,
}

impl fmt::Debug for DerivationPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let account = if let Some(account) = &self.account {
            format!("/{:?}", account)
        } else {
            "".to_string()
        };
        let change = if let Some(change) = &self.change {
            format!("/{:?}", change)
        } else {
            "".to_string()
        };
        write!(f, "m/44'/501'{}{}", account, change)
    }
}

impl DerivationPath {
    pub fn get_query(&self) -> String {
        if let Some(account) = &self.account {
            if let Some(change) = &self.change {
                format!("?key={}/{}", account, change)
            } else {
                format!("?key={}", account)
            }
        } else {
            "".to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_query() {
        let derivation_path = DerivationPath {
            account: None,
            change: None,
        };
        assert_eq!(derivation_path.get_query(), "".to_string());
        let derivation_path = DerivationPath {
            account: Some(1.into()),
            change: None,
        };
        assert_eq!(
            derivation_path.get_query(),
            format!("?key={}", DerivationPathComponent::from(1))
        );
        let derivation_path = DerivationPath {
            account: Some(1.into()),
            change: Some(2.into()),
        };
        assert_eq!(
            derivation_path.get_query(),
            format!(
                "?key={}/{}",
                DerivationPathComponent::from(1),
                DerivationPathComponent::from(2)
            )
        );
    }

    #[test]
    fn test_derivation_path_debug() {
        let mut path = DerivationPath::default();
        assert_eq!(format!("{:?}", path), "m/44'/501'".to_string());

        path.account = Some(1.into());
        assert_eq!(format!("{:?}", path), "m/44'/501'/1'".to_string());

        path.change = Some(2.into());
        assert_eq!(format!("{:?}", path), "m/44'/501'/1'/2'".to_string());
    }

    #[test]
    fn test_derivation_path_component() {
        let f = DerivationPathComponent::from(1);
        assert_eq!(f.as_u32(), 1 | DerivationPathComponent::HARDENED_BIT);

        let fs = DerivationPathComponent::from_str("1").unwrap();
        assert_eq!(fs, f);

        let fs = DerivationPathComponent::from_str("1'").unwrap();
        assert_eq!(fs, f);

        assert!(DerivationPathComponent::from_str("-1").is_err());

        assert_eq!(format!("{}", f), "1'".to_string());
        assert_eq!(format!("{:?}", f), "1'".to_string());
    }
}
