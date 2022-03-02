use {
    enum_iterator::IntoEnumIterator,
    log::*,
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount},
        pubkey::Pubkey,
        rent::Rent,
        transaction::{Result, TransactionError},
        transaction_context::TransactionContext,
    },
};

#[derive(Debug, PartialEq, IntoEnumIterator)]
pub(crate) enum RentState {
    Uninitialized, // account.lamports == 0
    RentPaying,    // 0 < account.lamports < rent-exempt-minimum
    RentExempt,    // account.lamports >= rent-exempt-minimum
}

impl RentState {
    pub(crate) fn from_account(account: &AccountSharedData, rent: &Rent) -> Self {
        if account.lamports() == 0 {
            Self::Uninitialized
        } else if !rent.is_exempt(account.lamports(), account.data().len()) {
            Self::RentPaying
        } else {
            Self::RentExempt
        }
    }

    pub(crate) fn transition_allowed_from(&self, pre_rent_state: &RentState) -> bool {
        // Only a legacy RentPaying account may end in the RentPaying state after message processing
        !(self == &Self::RentPaying && pre_rent_state != &Self::RentPaying)
    }
}

pub(crate) fn submit_rent_state_metrics(pre_rent_state: &RentState, post_rent_state: &RentState) {
    match (pre_rent_state, post_rent_state) {
        (&RentState::Uninitialized, &RentState::RentPaying) => {
            inc_new_counter_info!("rent_paying_err-new_account", 1);
        }
        (&RentState::RentPaying, &RentState::RentPaying) => {
            inc_new_counter_info!("rent_paying_ok-legacy", 1);
        }
        (_, &RentState::RentPaying) => {
            inc_new_counter_info!("rent_paying_err-other", 1);
        }
        _ => {}
    }
}

pub(crate) fn check_rent_state(
    pre_rent_state: Option<&RentState>,
    post_rent_state: Option<&RentState>,
    transaction_context: &TransactionContext,
    index: usize,
) -> Result<()> {
    if let Some((pre_rent_state, post_rent_state)) = pre_rent_state.zip(post_rent_state) {
        let expect_msg = "account must exist at TransactionContext index if rent-states are Some";
        check_rent_state_with_account(
            pre_rent_state,
            post_rent_state,
            transaction_context
                .get_key_of_account_at_index(index)
                .expect(expect_msg),
            &transaction_context
                .get_account_at_index(index)
                .expect(expect_msg)
                .borrow(),
        )?;
    }
    Ok(())
}

pub(crate) fn check_rent_state_with_account(
    pre_rent_state: &RentState,
    post_rent_state: &RentState,
    address: &Pubkey,
    account_state: &AccountSharedData,
) -> Result<()> {
    submit_rent_state_metrics(pre_rent_state, post_rent_state);
    if !solana_sdk::incinerator::check_id(address)
        && !post_rent_state.transition_allowed_from(pre_rent_state)
    {
        debug!(
            "Account {} not rent exempt, state {:?}",
            address, account_state,
        );
        return Err(TransactionError::InvalidRentPayingAccount);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use {super::*, solana_sdk::pubkey::Pubkey};

    #[test]
    fn test_from_account() {
        let program_id = Pubkey::new_unique();
        let uninitialized_account = AccountSharedData::new(0, 0, &Pubkey::default());

        let account_data_size = 100;

        let rent = Rent::free();
        let rent_exempt_account = AccountSharedData::new(1, account_data_size, &program_id); // if rent is free, all accounts with non-zero lamports and non-empty data are rent-exempt

        assert_eq!(
            RentState::from_account(&uninitialized_account, &rent),
            RentState::Uninitialized
        );
        assert_eq!(
            RentState::from_account(&rent_exempt_account, &rent),
            RentState::RentExempt
        );

        let rent = Rent::default();
        let rent_minimum_balance = rent.minimum_balance(account_data_size);
        let rent_paying_account = AccountSharedData::new(
            rent_minimum_balance.saturating_sub(1),
            account_data_size,
            &program_id,
        );
        let rent_exempt_account = AccountSharedData::new(
            rent.minimum_balance(account_data_size),
            account_data_size,
            &program_id,
        );

        assert_eq!(
            RentState::from_account(&uninitialized_account, &rent),
            RentState::Uninitialized
        );
        assert_eq!(
            RentState::from_account(&rent_paying_account, &rent),
            RentState::RentPaying
        );
        assert_eq!(
            RentState::from_account(&rent_exempt_account, &rent),
            RentState::RentExempt
        );
    }

    #[test]
    fn test_transition_allowed_from() {
        for post_rent_state in RentState::into_enum_iter() {
            for pre_rent_state in RentState::into_enum_iter() {
                if post_rent_state == RentState::RentPaying
                    && pre_rent_state != RentState::RentPaying
                {
                    assert!(!post_rent_state.transition_allowed_from(&pre_rent_state));
                } else {
                    assert!(post_rent_state.transition_allowed_from(&pre_rent_state));
                }
            }
        }
    }
}
