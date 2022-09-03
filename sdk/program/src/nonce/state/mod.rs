//! State for durable transaction nonces.

mod current;
pub use current::{Data, DurableNonce, State};
use {
    crate::{hash::Hash, pubkey::Pubkey},
    serde_derive::{Deserialize, Serialize},
    std::collections::HashSet,
};

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub enum Versions {
    Legacy(Box<State>),
    /// Current variants have durable nonce and blockhash domains separated.
    Current(Box<State>),
}

#[derive(Debug, Eq, PartialEq)]
pub enum AuthorizeNonceError {
    MissingRequiredSignature(/*account authority:*/ Pubkey),
    Uninitialized,
}

impl Versions {
    pub fn new(state: State) -> Self {
        Self::Current(Box::new(state))
    }

    pub fn state(&self) -> &State {
        match self {
            Self::Legacy(state) => state,
            Self::Current(state) => state,
        }
    }

    /// Checks if the recent_blockhash field in Transaction verifies, and
    /// returns nonce account data if so.
    pub fn verify_recent_blockhash(
        &self,
        recent_blockhash: &Hash, // Transaction.message.recent_blockhash
    ) -> Option<&Data> {
        match self {
            // Legacy durable nonces are invalid and should not
            // allow durable transactions.
            Self::Legacy(_) => None,
            Self::Current(state) => match **state {
                State::Uninitialized => None,
                State::Initialized(ref data) => {
                    (recent_blockhash == &data.blockhash()).then_some(data)
                }
            },
        }
    }

    /// Upgrades legacy nonces out of chain blockhash domains.
    pub fn upgrade(self) -> Option<Self> {
        match self {
            Self::Legacy(mut state) => {
                match *state {
                    // An Uninitialized legacy nonce cannot verify a durable
                    // transaction. The nonce will be upgraded to Current
                    // version when initialized. Therefore there is no need to
                    // upgrade Uninitialized legacy nonces.
                    State::Uninitialized => None,
                    State::Initialized(ref mut data) => {
                        data.durable_nonce = DurableNonce::from_blockhash(&data.blockhash());
                        Some(Self::Current(state))
                    }
                }
            }
            Self::Current(_) => None,
        }
    }

    /// Updates the authority pubkey on the nonce account.
    pub fn authorize(
        self,
        signers: &HashSet<Pubkey>,
        authority: Pubkey,
    ) -> Result<Self, AuthorizeNonceError> {
        let data = match self.state() {
            State::Uninitialized => return Err(AuthorizeNonceError::Uninitialized),
            State::Initialized(data) => data,
        };
        if !signers.contains(&data.authority) {
            return Err(AuthorizeNonceError::MissingRequiredSignature(
                data.authority,
            ));
        }
        let data = Data::new(
            authority,
            data.durable_nonce,
            data.get_lamports_per_signature(),
        );
        let state = Box::new(State::Initialized(data));
        // Preserve Version variant since cannot
        // change durable_nonce field here.
        Ok(match self {
            Self::Legacy(_) => Self::Legacy,
            Self::Current(_) => Self::Current,
        }(state))
    }
}

impl From<Versions> for State {
    fn from(versions: Versions) -> Self {
        match versions {
            Versions::Legacy(state) => *state,
            Versions::Current(state) => *state,
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{fee_calculator::FeeCalculator, pubkey::Pubkey},
        std::iter::repeat_with,
    };

    #[test]
    fn test_verify_recent_blockhash() {
        let blockhash = Hash::from([171; 32]);
        let versions = Versions::Legacy(Box::new(State::Uninitialized));
        assert_eq!(versions.verify_recent_blockhash(&blockhash), None);
        assert_eq!(versions.verify_recent_blockhash(&Hash::default()), None);
        let versions = Versions::Current(Box::new(State::Uninitialized));
        assert_eq!(versions.verify_recent_blockhash(&blockhash), None);
        assert_eq!(versions.verify_recent_blockhash(&Hash::default()), None);
        let durable_nonce = DurableNonce::from_blockhash(&blockhash);
        let data = Data {
            authority: Pubkey::new_unique(),
            durable_nonce,
            fee_calculator: FeeCalculator {
                lamports_per_signature: 2718,
            },
        };
        let versions = Versions::Legacy(Box::new(State::Initialized(data.clone())));
        assert_eq!(versions.verify_recent_blockhash(&Hash::default()), None);
        assert_eq!(versions.verify_recent_blockhash(&blockhash), None);
        assert_eq!(versions.verify_recent_blockhash(&data.blockhash()), None);
        assert_eq!(
            versions.verify_recent_blockhash(durable_nonce.as_hash()),
            None
        );
        let durable_nonce = DurableNonce::from_blockhash(durable_nonce.as_hash());
        assert_ne!(data.durable_nonce, durable_nonce);
        let data = Data {
            durable_nonce,
            ..data
        };
        let versions = Versions::Current(Box::new(State::Initialized(data.clone())));
        assert_eq!(versions.verify_recent_blockhash(&blockhash), None);
        assert_eq!(versions.verify_recent_blockhash(&Hash::default()), None);
        assert_eq!(
            versions.verify_recent_blockhash(&data.blockhash()),
            Some(&data)
        );
        assert_eq!(
            versions.verify_recent_blockhash(durable_nonce.as_hash()),
            Some(&data)
        );
    }

    #[test]
    fn test_nonce_versions_upgrade() {
        // Uninitialized
        let versions = Versions::Legacy(Box::new(State::Uninitialized));
        assert_eq!(versions.upgrade(), None);
        // Initialized
        let blockhash = Hash::from([171; 32]);
        let durable_nonce = DurableNonce::from_blockhash(&blockhash);
        let data = Data {
            authority: Pubkey::new_unique(),
            durable_nonce,
            fee_calculator: FeeCalculator {
                lamports_per_signature: 2718,
            },
        };
        let versions = Versions::Legacy(Box::new(State::Initialized(data.clone())));
        let durable_nonce = DurableNonce::from_blockhash(durable_nonce.as_hash());
        assert_ne!(data.durable_nonce, durable_nonce);
        let data = Data {
            durable_nonce,
            ..data
        };
        let versions = versions.upgrade().unwrap();
        assert_eq!(
            versions,
            Versions::Current(Box::new(State::Initialized(data)))
        );
        assert_eq!(versions.upgrade(), None);
    }

    #[test]
    fn test_nonce_versions_authorize() {
        // Uninitialized
        let mut signers = repeat_with(Pubkey::new_unique).take(16).collect();
        let versions = Versions::Legacy(Box::new(State::Uninitialized));
        assert_eq!(
            versions.authorize(&signers, Pubkey::new_unique()),
            Err(AuthorizeNonceError::Uninitialized)
        );
        let versions = Versions::Current(Box::new(State::Uninitialized));
        assert_eq!(
            versions.authorize(&signers, Pubkey::new_unique()),
            Err(AuthorizeNonceError::Uninitialized)
        );
        // Initialized, Legacy
        let blockhash = Hash::from([171; 32]);
        let durable_nonce = DurableNonce::from_blockhash(&blockhash);
        let data = Data {
            authority: Pubkey::new_unique(),
            durable_nonce,
            fee_calculator: FeeCalculator {
                lamports_per_signature: 2718,
            },
        };
        let account_authority = data.authority;
        let versions = Versions::Legacy(Box::new(State::Initialized(data.clone())));
        let authority = Pubkey::new_unique();
        assert_ne!(authority, account_authority);
        let data = Data { authority, ..data };
        assert_eq!(
            versions.clone().authorize(&signers, authority),
            Err(AuthorizeNonceError::MissingRequiredSignature(
                account_authority
            )),
        );
        assert!(signers.insert(account_authority));
        assert_eq!(
            versions.authorize(&signers, authority),
            Ok(Versions::Legacy(Box::new(State::Initialized(data.clone()))))
        );
        // Initialized, Current
        let account_authority = data.authority;
        let versions = Versions::Current(Box::new(State::Initialized(data.clone())));
        let authority = Pubkey::new_unique();
        assert_ne!(authority, account_authority);
        let data = Data { authority, ..data };
        assert_eq!(
            versions.clone().authorize(&signers, authority),
            Err(AuthorizeNonceError::MissingRequiredSignature(
                account_authority
            )),
        );
        assert!(signers.insert(account_authority));
        assert_eq!(
            versions.authorize(&signers, authority),
            Ok(Versions::Current(Box::new(State::Initialized(data))))
        );
    }
}
