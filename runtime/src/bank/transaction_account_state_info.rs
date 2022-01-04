use {
    crate::{
        account_rent_state::{submit_rent_state_metrics, RentState},
        bank::Bank,
        message_processor::ProcessedMessageInfo,
    },
    log::debug,
    solana_sdk::{
        transaction::{Result, TransactionError},
        transaction_context::TransactionContext,
    },
};

pub(crate) struct TransactionAccountStateInfo {
    rent_state: RentState,
}

impl Bank {
    pub(crate) fn get_transaction_account_state_info(
        &self,
        transaction_context: &TransactionContext,
    ) -> Vec<TransactionAccountStateInfo> {
        (0..transaction_context.get_number_of_accounts())
            .map(|i| {
                let account = transaction_context.get_account_at_index(i).borrow();
                TransactionAccountStateInfo {
                    rent_state: RentState::from_account(&account, &self.rent_collector().rent),
                }
            })
            .collect()
    }

    pub(crate) fn verify_transaction_account_state_changes(
        process_result: &mut Result<ProcessedMessageInfo>,
        pre_state_infos: &[TransactionAccountStateInfo],
        post_state_infos: &[TransactionAccountStateInfo],
        transaction_context: &TransactionContext,
    ) {
        if process_result.is_ok() {
            for (i, (pre_state_info, post_state_info)) in
                pre_state_infos.iter().zip(post_state_infos).enumerate()
            {
                if !post_state_info
                    .rent_state
                    .transition_allowed_from(&pre_state_info.rent_state)
                {
                    debug!(
                        "Account {:?} not rent exempt, state {:?}",
                        transaction_context.get_key_of_account_at_index(i),
                        transaction_context.get_account_at_index(i).borrow(),
                    );
                    submit_rent_state_metrics(
                        &pre_state_info.rent_state,
                        &post_state_info.rent_state,
                    );
                    *process_result = Err(TransactionError::InvalidRentPayingAccount)
                }
            }
        }
    }
}
