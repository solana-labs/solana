use {
    crate::bank::TransactionAccountRefCell,
    enum_iterator::IntoEnumIterator,
    solana_sdk::{account::ReadableAccount, native_loader, rent::Rent, sysvar},
};

#[derive(Debug, PartialEq, IntoEnumIterator)]
pub(crate) enum RentState {
    Uninitialized, // account.lamports == 0
    NativeOrSysvar,
    EmptyData,  // account.data.len == 0
    RentPaying, // 0 < account.lamports < rent-exempt-minimum for non-zero data size
    RentExempt, // account.lamports >= rent-exempt-minimum for non-zero data size
}

impl RentState {
    pub(crate) fn from_account(account: &TransactionAccountRefCell, rent: &Rent) -> Self {
        let account = account.1.borrow();
        if account.lamports() == 0 {
            Self::Uninitialized
        } else if native_loader::check_id(account.owner()) || sysvar::is_sysvar_id(account.owner())
        {
            Self::NativeOrSysvar
        } else if account.data().is_empty() {
            Self::EmptyData
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

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_sdk::{account::AccountSharedData, pubkey::Pubkey, system_program},
        std::{cell::RefCell, rc::Rc},
    };

    #[test]
    fn test_from_account() {
        let account_address = Pubkey::new_unique();
        let program_id = Pubkey::new_unique();
        let uninitialized_account = Rc::new(RefCell::new(AccountSharedData::new(
            0,
            0,
            &Pubkey::default(),
        )));
        let native_program_account = Rc::new(RefCell::new(AccountSharedData::new(
            1,
            42,
            &native_loader::id(),
        )));
        let sysvar_account = Rc::new(RefCell::new(AccountSharedData::new(
            1,
            42,
            &sysvar::clock::id(),
        )));
        let empty_data_system_account = Rc::new(RefCell::new(AccountSharedData::new(
            10,
            0,
            &system_program::id(),
        )));
        let empty_data_other_account =
            Rc::new(RefCell::new(AccountSharedData::new(10, 0, &program_id)));

        let account_data_size = 100;

        let rent = Rent::free();
        let rent_exempt_account = Rc::new(RefCell::new(AccountSharedData::new(
            1,
            account_data_size,
            &program_id,
        ))); // if rent is free, all accounts with non-zero lamports and non-empty data are rent-exempt

        assert_eq!(
            RentState::from_account(&(account_address, uninitialized_account.clone()), &rent),
            RentState::Uninitialized
        );
        assert_eq!(
            RentState::from_account(&(account_address, native_program_account.clone()), &rent),
            RentState::NativeOrSysvar
        );
        assert_eq!(
            RentState::from_account(&(account_address, sysvar_account.clone()), &rent),
            RentState::NativeOrSysvar
        );
        assert_eq!(
            RentState::from_account(&(account_address, empty_data_system_account.clone()), &rent),
            RentState::EmptyData
        );
        assert_eq!(
            RentState::from_account(&(account_address, empty_data_other_account.clone()), &rent),
            RentState::EmptyData
        );
        assert_eq!(
            RentState::from_account(&(account_address, rent_exempt_account), &rent),
            RentState::RentExempt
        );

        let rent = Rent::default();
        let rent_minimum_balance = rent.minimum_balance(account_data_size);
        let rent_paying_account = Rc::new(RefCell::new(AccountSharedData::new(
            rent_minimum_balance.saturating_sub(1),
            account_data_size,
            &program_id,
        )));
        let rent_exempt_account = Rc::new(RefCell::new(AccountSharedData::new(
            rent.minimum_balance(account_data_size),
            account_data_size,
            &program_id,
        )));

        assert_eq!(
            RentState::from_account(&(account_address, uninitialized_account), &rent),
            RentState::Uninitialized
        );
        assert_eq!(
            RentState::from_account(&(account_address, native_program_account), &rent),
            RentState::NativeOrSysvar
        );
        assert_eq!(
            RentState::from_account(&(account_address, sysvar_account), &rent),
            RentState::NativeOrSysvar
        );
        assert_eq!(
            RentState::from_account(&(account_address, empty_data_system_account), &rent),
            RentState::EmptyData
        );
        assert_eq!(
            RentState::from_account(&(account_address, empty_data_other_account), &rent),
            RentState::EmptyData
        );
        assert_eq!(
            RentState::from_account(&(account_address, rent_paying_account), &rent),
            RentState::RentPaying
        );
        assert_eq!(
            RentState::from_account(&(account_address, rent_exempt_account), &rent),
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
