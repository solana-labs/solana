use {
    crate::rent_debits::RentDebits,
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount, WritableAccount},
        message::SanitizedMessage,
        nonce_account,
        pubkey::Pubkey,
        transaction::{self, TransactionError},
        transaction_context::TransactionAccount,
    },
};

pub trait NonceInfo {
    fn address(&self) -> &Pubkey;
    fn account(&self) -> &AccountSharedData;
    fn lamports_per_signature(&self) -> Option<u64>;
    fn fee_payer_account(&self) -> Option<&AccountSharedData>;
}

/// Holds limited nonce info available during transaction checks
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NoncePartial {
    address: Pubkey,
    account: AccountSharedData,
}

impl NoncePartial {
    pub fn new(address: Pubkey, account: AccountSharedData) -> Self {
        Self { address, account }
    }
}

impl NonceInfo for NoncePartial {
    fn address(&self) -> &Pubkey {
        &self.address
    }
    fn account(&self) -> &AccountSharedData {
        &self.account
    }
    fn lamports_per_signature(&self) -> Option<u64> {
        nonce_account::lamports_per_signature_of(&self.account)
    }
    fn fee_payer_account(&self) -> Option<&AccountSharedData> {
        None
    }
}

/// Holds fee subtracted nonce info
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NonceFull {
    address: Pubkey,
    account: AccountSharedData,
    fee_payer_account: Option<AccountSharedData>,
}

impl NonceFull {
    pub fn new(
        address: Pubkey,
        account: AccountSharedData,
        fee_payer_account: Option<AccountSharedData>,
    ) -> Self {
        Self {
            address,
            account,
            fee_payer_account,
        }
    }
    pub fn from_partial(
        partial: NoncePartial,
        message: &SanitizedMessage,
        accounts: &[TransactionAccount],
        rent_debits: &RentDebits,
    ) -> transaction::Result<Self> {
        let fee_payer = (0..message.account_keys().len()).find_map(|i| {
            if let Some((k, a)) = &accounts.get(i) {
                if message.is_non_loader_key(i) {
                    return Some((k, a));
                }
            }
            None
        });

        if let Some((fee_payer_address, fee_payer_account)) = fee_payer {
            let mut fee_payer_account = fee_payer_account.clone();
            let rent_debit = rent_debits.get_account_rent_debit(fee_payer_address);
            fee_payer_account.set_lamports(fee_payer_account.lamports().saturating_add(rent_debit));

            let nonce_address = *partial.address();
            if *fee_payer_address == nonce_address {
                Ok(Self::new(nonce_address, fee_payer_account, None))
            } else {
                Ok(Self::new(
                    nonce_address,
                    partial.account().clone(),
                    Some(fee_payer_account),
                ))
            }
        } else {
            Err(TransactionError::AccountNotFound)
        }
    }
}

impl NonceInfo for NonceFull {
    fn address(&self) -> &Pubkey {
        &self.address
    }
    fn account(&self) -> &AccountSharedData {
        &self.account
    }
    fn lamports_per_signature(&self) -> Option<u64> {
        nonce_account::lamports_per_signature_of(&self.account)
    }
    fn fee_payer_account(&self) -> Option<&AccountSharedData> {
        self.fee_payer_account.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_sdk::{
            hash::Hash,
            instruction::Instruction,
            message::Message,
            nonce::{self, state::DurableNonce},
            signature::{keypair_from_seed, Signer},
            system_instruction, system_program,
        },
    };

    fn new_sanitized_message(
        instructions: &[Instruction],
        payer: Option<&Pubkey>,
    ) -> SanitizedMessage {
        Message::new(instructions, payer).try_into().unwrap()
    }

    #[test]
    fn test_nonce_info() {
        let lamports_per_signature = 42;

        let nonce_authority = keypair_from_seed(&[0; 32]).unwrap();
        let nonce_address = nonce_authority.pubkey();
        let from = keypair_from_seed(&[1; 32]).unwrap();
        let from_address = from.pubkey();
        let to_address = Pubkey::new_unique();

        let durable_nonce = DurableNonce::from_blockhash(&Hash::new_unique());
        let nonce_account = AccountSharedData::new_data(
            43,
            &nonce::state::Versions::new(nonce::State::Initialized(nonce::state::Data::new(
                Pubkey::default(),
                durable_nonce,
                lamports_per_signature,
            ))),
            &system_program::id(),
        )
        .unwrap();
        let from_account = AccountSharedData::new(44, 0, &Pubkey::default());
        let to_account = AccountSharedData::new(45, 0, &Pubkey::default());
        let recent_blockhashes_sysvar_account = AccountSharedData::new(4, 0, &Pubkey::default());

        const TEST_RENT_DEBIT: u64 = 1;
        let rent_collected_nonce_account = {
            let mut account = nonce_account.clone();
            account.set_lamports(nonce_account.lamports() - TEST_RENT_DEBIT);
            account
        };
        let rent_collected_from_account = {
            let mut account = from_account.clone();
            account.set_lamports(from_account.lamports() - TEST_RENT_DEBIT);
            account
        };

        let instructions = vec![
            system_instruction::advance_nonce_account(&nonce_address, &nonce_authority.pubkey()),
            system_instruction::transfer(&from_address, &to_address, 42),
        ];

        // NoncePartial create + NonceInfo impl
        let partial = NoncePartial::new(nonce_address, rent_collected_nonce_account.clone());
        assert_eq!(*partial.address(), nonce_address);
        assert_eq!(*partial.account(), rent_collected_nonce_account);
        assert_eq!(
            partial.lamports_per_signature(),
            Some(lamports_per_signature)
        );
        assert_eq!(partial.fee_payer_account(), None);

        // Add rent debits to ensure the rollback captures accounts without rent fees
        let mut rent_debits = RentDebits::default();
        rent_debits.insert(
            &from_address,
            TEST_RENT_DEBIT,
            rent_collected_from_account.lamports(),
        );
        rent_debits.insert(
            &nonce_address,
            TEST_RENT_DEBIT,
            rent_collected_nonce_account.lamports(),
        );

        // NonceFull create + NonceInfo impl
        {
            let message = new_sanitized_message(&instructions, Some(&from_address));
            let accounts = [
                (
                    *message.account_keys().get(0).unwrap(),
                    rent_collected_from_account.clone(),
                ),
                (
                    *message.account_keys().get(1).unwrap(),
                    rent_collected_nonce_account.clone(),
                ),
                (*message.account_keys().get(2).unwrap(), to_account.clone()),
                (
                    *message.account_keys().get(3).unwrap(),
                    recent_blockhashes_sysvar_account.clone(),
                ),
            ];

            let full = NonceFull::from_partial(partial.clone(), &message, &accounts, &rent_debits)
                .unwrap();
            assert_eq!(*full.address(), nonce_address);
            assert_eq!(*full.account(), rent_collected_nonce_account);
            assert_eq!(full.lamports_per_signature(), Some(lamports_per_signature));
            assert_eq!(
                full.fee_payer_account(),
                Some(&from_account),
                "rent debit should be refunded in captured fee account"
            );
        }

        // Nonce account is fee-payer
        {
            let message = new_sanitized_message(&instructions, Some(&nonce_address));
            let accounts = [
                (
                    *message.account_keys().get(0).unwrap(),
                    rent_collected_nonce_account,
                ),
                (
                    *message.account_keys().get(1).unwrap(),
                    rent_collected_from_account,
                ),
                (*message.account_keys().get(2).unwrap(), to_account),
                (
                    *message.account_keys().get(3).unwrap(),
                    recent_blockhashes_sysvar_account,
                ),
            ];

            let full = NonceFull::from_partial(partial.clone(), &message, &accounts, &rent_debits)
                .unwrap();
            assert_eq!(*full.address(), nonce_address);
            assert_eq!(*full.account(), nonce_account);
            assert_eq!(full.lamports_per_signature(), Some(lamports_per_signature));
            assert_eq!(full.fee_payer_account(), None);
        }

        // NonceFull create, fee-payer not in account_keys fails
        {
            let message = new_sanitized_message(&instructions, Some(&nonce_address));
            assert_eq!(
                NonceFull::from_partial(partial, &message, &[], &RentDebits::default())
                    .unwrap_err(),
                TransactionError::AccountNotFound,
            );
        }
    }
}
