use {
    crate::nonce_info::NonceInfo,
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount, WritableAccount},
        clock::Epoch,
        pubkey::Pubkey,
    },
};

/// Captured account state used to rollback account state for nonce and fee
/// payer accounts after a failed executed transaction.
#[derive(PartialEq, Eq, Debug, Clone)]
pub enum RollbackAccounts {
    FeePayerOnly {
        fee_payer_account: AccountSharedData,
    },
    SameNonceAndFeePayer {
        nonce: NonceInfo,
    },
    SeparateNonceAndFeePayer {
        nonce: NonceInfo,
        fee_payer_account: AccountSharedData,
    },
}

#[cfg(feature = "dev-context-only-utils")]
impl Default for RollbackAccounts {
    fn default() -> Self {
        Self::FeePayerOnly {
            fee_payer_account: AccountSharedData::default(),
        }
    }
}

impl RollbackAccounts {
    pub fn new(
        nonce: Option<NonceInfo>,
        fee_payer_address: Pubkey,
        mut fee_payer_account: AccountSharedData,
        fee_payer_rent_debit: u64,
        fee_payer_loaded_rent_epoch: Epoch,
    ) -> Self {
        // When the fee payer account is rolled back due to transaction failure,
        // rent should not be charged so credit the previously debited rent
        // amount.
        fee_payer_account.set_lamports(
            fee_payer_account
                .lamports()
                .saturating_add(fee_payer_rent_debit),
        );

        if let Some(nonce) = nonce {
            if &fee_payer_address == nonce.address() {
                RollbackAccounts::SameNonceAndFeePayer {
                    nonce: NonceInfo::new(fee_payer_address, fee_payer_account),
                }
            } else {
                RollbackAccounts::SeparateNonceAndFeePayer {
                    nonce,
                    fee_payer_account,
                }
            }
        } else {
            // When rolling back failed transactions which don't use nonces, the
            // runtime should not update the fee payer's rent epoch so reset the
            // rollback fee payer acocunt's rent epoch to its originally loaded
            // rent epoch value. In the future, a feature gate could be used to
            // alter this behavior such that rent epoch updates are handled the
            // same for both nonce and non-nonce failed transactions.
            fee_payer_account.set_rent_epoch(fee_payer_loaded_rent_epoch);
            RollbackAccounts::FeePayerOnly { fee_payer_account }
        }
    }

    /// Number of accounts tracked for rollback
    pub fn count(&self) -> usize {
        match self {
            Self::FeePayerOnly { .. } | Self::SameNonceAndFeePayer { .. } => 1,
            Self::SeparateNonceAndFeePayer { .. } => 2,
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_sdk::{
            account::{ReadableAccount, WritableAccount},
            hash::Hash,
            nonce::state::{
                Data as NonceData, DurableNonce, State as NonceState, Versions as NonceVersions,
            },
            system_program,
        },
    };

    #[test]
    fn test_new_fee_payer_only() {
        let fee_payer_address = Pubkey::new_unique();
        let fee_payer_account = AccountSharedData::new(100, 0, &Pubkey::default());
        let fee_payer_rent_epoch = fee_payer_account.rent_epoch();

        const TEST_RENT_DEBIT: u64 = 1;
        let rent_collected_fee_payer_account = {
            let mut account = fee_payer_account.clone();
            account.set_lamports(fee_payer_account.lamports() - TEST_RENT_DEBIT);
            account.set_rent_epoch(fee_payer_rent_epoch + 1);
            account
        };

        let rollback_accounts = RollbackAccounts::new(
            None,
            fee_payer_address,
            rent_collected_fee_payer_account,
            TEST_RENT_DEBIT,
            fee_payer_rent_epoch,
        );

        let expected_fee_payer_account = fee_payer_account;
        match rollback_accounts {
            RollbackAccounts::FeePayerOnly { fee_payer_account } => {
                assert_eq!(expected_fee_payer_account, fee_payer_account);
            }
            _ => panic!("Expected FeePayerOnly variant"),
        }
    }

    #[test]
    fn test_new_same_nonce_and_fee_payer() {
        let nonce_address = Pubkey::new_unique();
        let durable_nonce = DurableNonce::from_blockhash(&Hash::new_unique());
        let lamports_per_signature = 42;
        let nonce_account = AccountSharedData::new_data(
            43,
            &NonceVersions::new(NonceState::Initialized(NonceData::new(
                Pubkey::default(),
                durable_nonce,
                lamports_per_signature,
            ))),
            &system_program::id(),
        )
        .unwrap();

        const TEST_RENT_DEBIT: u64 = 1;
        let rent_collected_nonce_account = {
            let mut account = nonce_account.clone();
            account.set_lamports(nonce_account.lamports() - TEST_RENT_DEBIT);
            account
        };

        let nonce = NonceInfo::new(nonce_address, rent_collected_nonce_account.clone());
        let rollback_accounts = RollbackAccounts::new(
            Some(nonce),
            nonce_address,
            rent_collected_nonce_account,
            TEST_RENT_DEBIT,
            u64::MAX, // ignored
        );

        match rollback_accounts {
            RollbackAccounts::SameNonceAndFeePayer { nonce } => {
                assert_eq!(nonce.address(), &nonce_address);
                assert_eq!(nonce.account(), &nonce_account);
            }
            _ => panic!("Expected SameNonceAndFeePayer variant"),
        }
    }

    #[test]
    fn test_separate_nonce_and_fee_payer() {
        let nonce_address = Pubkey::new_unique();
        let durable_nonce = DurableNonce::from_blockhash(&Hash::new_unique());
        let lamports_per_signature = 42;
        let nonce_account = AccountSharedData::new_data(
            43,
            &NonceVersions::new(NonceState::Initialized(NonceData::new(
                Pubkey::default(),
                durable_nonce,
                lamports_per_signature,
            ))),
            &system_program::id(),
        )
        .unwrap();

        let fee_payer_address = Pubkey::new_unique();
        let fee_payer_account = AccountSharedData::new(44, 0, &Pubkey::default());

        const TEST_RENT_DEBIT: u64 = 1;
        let rent_collected_fee_payer_account = {
            let mut account = fee_payer_account.clone();
            account.set_lamports(fee_payer_account.lamports() - TEST_RENT_DEBIT);
            account
        };

        let nonce = NonceInfo::new(nonce_address, nonce_account.clone());
        let rollback_accounts = RollbackAccounts::new(
            Some(nonce),
            fee_payer_address,
            rent_collected_fee_payer_account.clone(),
            TEST_RENT_DEBIT,
            u64::MAX, // ignored
        );

        let expected_fee_payer_account = fee_payer_account;
        match rollback_accounts {
            RollbackAccounts::SeparateNonceAndFeePayer {
                nonce,
                fee_payer_account,
            } => {
                assert_eq!(nonce.address(), &nonce_address);
                assert_eq!(nonce.account(), &nonce_account);
                assert_eq!(expected_fee_payer_account, fee_payer_account);
            }
            _ => panic!("Expected SeparateNonceAndFeePayer variant"),
        }
    }
}
