use {
    crate::account_rent_state::RentState,
    solana_sdk::{
        account::ReadableAccount,
        message::SanitizedMessage,
        native_loader,
        rent::Rent,
        transaction::Result,
        transaction_context::{IndexOfAccount, TransactionContext},
    },
};

pub struct TransactionAccountStateInfo {
    rent_state: Option<RentState>, // None: readonly account
}

impl TransactionAccountStateInfo {
    pub fn new(
        rent: &Rent,
        transaction_context: &TransactionContext,
        message: &SanitizedMessage,
    ) -> Vec<Self> {
        (0..message.account_keys().len())
            .map(|i| {
                let rent_state = if message.is_writable(i) {
                    let state = if let Ok(account) =
                        transaction_context.get_account_at_index(i as IndexOfAccount)
                    {
                        let account = account.borrow();

                        // Native programs appear to be RentPaying because they carry low lamport
                        // balances; however they will never be loaded as writable
                        debug_assert!(!native_loader::check_id(account.owner()));

                        Some(RentState::from_account(&account, rent))
                    } else {
                        None
                    };
                    debug_assert!(
                        state.is_some(),
                        "message and transaction context out of sync, fatal"
                    );
                    state
                } else {
                    None
                };
                Self { rent_state }
            })
            .collect()
    }

    pub(crate) fn verify_changes(
        pre_state_infos: &[Self],
        post_state_infos: &[Self],
        transaction_context: &TransactionContext,
    ) -> Result<()> {
        for (i, (pre_state_info, post_state_info)) in
            pre_state_infos.iter().zip(post_state_infos).enumerate()
        {
            RentState::check_rent_state(
                pre_state_info.rent_state.as_ref(),
                post_state_info.rent_state.as_ref(),
                transaction_context,
                i as IndexOfAccount,
            )?;
        }
        Ok(())
    }
}
