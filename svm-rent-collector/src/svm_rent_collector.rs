//! Plugin trait for rent collection within the Solana SVM.

use {
    crate::rent_state::RentState,
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount},
        clock::Epoch,
        pubkey::Pubkey,
        rent::{Rent, RentDue},
        rent_collector::CollectedInfo,
        transaction::{Result, TransactionError},
        transaction_context::{IndexOfAccount, TransactionContext},
    },
};

mod rent_collector;

/// Rent collector trait. Represents an entity that can evaluate the rent state
/// of an account, determine rent due, and collect rent.
///
/// Implementors are responsible for evaluating rent due and collecting rent
/// from accounts, if required. Methods for evaluating account rent state have
/// default implementations, which can be overridden for customized rent
/// management.
pub trait SVMRentCollector {
    /// Check rent state transition for an account in a transaction.
    ///
    /// This method has a default implementation that calls into
    /// `check_rent_state_with_account`.
    fn check_rent_state(
        &self,
        pre_rent_state: Option<&RentState>,
        post_rent_state: Option<&RentState>,
        transaction_context: &TransactionContext,
        index: IndexOfAccount,
    ) -> Result<()> {
        if let Some((pre_rent_state, post_rent_state)) = pre_rent_state.zip(post_rent_state) {
            let expect_msg =
                "account must exist at TransactionContext index if rent-states are Some";
            self.check_rent_state_with_account(
                pre_rent_state,
                post_rent_state,
                transaction_context
                    .get_key_of_account_at_index(index)
                    .expect(expect_msg),
                &transaction_context
                    .get_account_at_index(index)
                    .expect(expect_msg)
                    .borrow(),
                index,
            )?;
        }
        Ok(())
    }

    /// Check rent state transition for an account directly.
    ///
    /// This method has a default implementation that checks whether the
    /// transition is allowed and returns an error if it is not. It also
    /// verifies that the account is not the incinerator.
    fn check_rent_state_with_account(
        &self,
        pre_rent_state: &RentState,
        post_rent_state: &RentState,
        address: &Pubkey,
        _account_state: &AccountSharedData,
        account_index: IndexOfAccount,
    ) -> Result<()> {
        if !solana_sdk::incinerator::check_id(address)
            && !self.transition_allowed(pre_rent_state, post_rent_state)
        {
            let account_index = account_index as u8;
            Err(TransactionError::InsufficientFundsForRent { account_index })
        } else {
            Ok(())
        }
    }

    /// Collect rent from an account.
    fn collect_rent(&self, address: &Pubkey, account: &mut AccountSharedData) -> CollectedInfo;

    /// Determine the rent state of an account.
    ///
    /// This method has a default implementation that treats accounts with zero
    /// lamports as uninitialized and uses the implemented `get_rent` to
    /// determine whether an account is rent-exempt.
    fn get_account_rent_state(&self, account: &AccountSharedData) -> RentState {
        if account.lamports() == 0 {
            RentState::Uninitialized
        } else if self
            .get_rent()
            .is_exempt(account.lamports(), account.data().len())
        {
            RentState::RentExempt
        } else {
            RentState::RentPaying {
                data_size: account.data().len(),
                lamports: account.lamports(),
            }
        }
    }

    /// Get the rent collector's rent instance.
    fn get_rent(&self) -> &Rent;

    /// Get the rent due for an account.
    fn get_rent_due(&self, lamports: u64, data_len: usize, account_rent_epoch: Epoch) -> RentDue;

    /// Check whether a transition from the pre_rent_state to the
    /// post_rent_state is valid.
    ///
    /// This method has a default implementation that allows transitions from
    /// any state to `RentState::Uninitialized` or `RentState::RentExempt`.
    /// Pre-state `RentState::RentPaying` can only transition to
    /// `RentState::RentPaying` if the data size remains the same and the
    /// account is not credited.
    fn transition_allowed(&self, pre_rent_state: &RentState, post_rent_state: &RentState) -> bool {
        match post_rent_state {
            RentState::Uninitialized | RentState::RentExempt => true,
            RentState::RentPaying {
                data_size: post_data_size,
                lamports: post_lamports,
            } => {
                match pre_rent_state {
                    RentState::Uninitialized | RentState::RentExempt => false,
                    RentState::RentPaying {
                        data_size: pre_data_size,
                        lamports: pre_lamports,
                    } => {
                        // Cannot remain RentPaying if resized or credited.
                        post_data_size == pre_data_size && post_lamports <= pre_lamports
                    }
                }
            }
        }
    }
}
