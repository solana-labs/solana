use {
    crate::bank::Bank,
    solana_accounts_db::account_rent_state::{check_rent_state, RentState},
    solana_sdk::{
        account::ReadableAccount,
        message::SanitizedMessage,
        native_loader,
        transaction::Result,
        transaction_context::{IndexOfAccount, TransactionContext},
    },
};

pub(crate) struct TransactionAccountStateInfo {
    rent_state: Option<RentState>, // None: readonly account
}

impl Bank {
    pub(crate) fn get_transaction_account_state_info(
        &self,
        transaction_context: &TransactionContext,
        message: &SanitizedMessage,
    ) -> Vec<TransactionAccountStateInfo> {
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

                        Some(RentState::from_account(
                            &account,
                            &self.rent_collector().rent,
                        ))
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
                TransactionAccountStateInfo { rent_state }
            })
            .collect()
    }

    pub(crate) fn verify_transaction_account_state_changes(
        &self,
        pre_state_infos: &[TransactionAccountStateInfo],
        post_state_infos: &[TransactionAccountStateInfo],
        transaction_context: &TransactionContext,
    ) -> Result<()> {
        for (i, (pre_state_info, post_state_info)) in
            pre_state_infos.iter().zip(post_state_infos).enumerate()
        {
            check_rent_state(
                pre_state_info.rent_state.as_ref(),
                post_state_info.rent_state.as_ref(),
                transaction_context,
                i as IndexOfAccount,
            )?;
        }
        Ok(())
    }
}
