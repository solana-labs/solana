use std::cmp::min;

use crate::instruction::ApplicationFeesInstuctions;

use {
    solana_program_runtime::{ic_msg, invoke_context::InvokeContext},
    solana_sdk::{
        account::ReadableAccount, instruction::InstructionError,
        program_utils::limited_deserialize, transaction_context::IndexOfAccount,
    },
};

pub fn process_instruction(
    _first_instruction_account: IndexOfAccount,
    invoke_context: &mut InvokeContext,
) -> Result<(), InstructionError> {
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;
    let instruction_data = instruction_context.get_instruction_data();

    match limited_deserialize(instruction_data)? {
        ApplicationFeesInstuctions::UpdateFees { fees } => {
            Processor::add_or_update_fees(invoke_context, fees)
        }
        ApplicationFeesInstuctions::Rebate { rebate_fees } => {
            Processor::rebate(invoke_context, rebate_fees)
        }
        ApplicationFeesInstuctions::RebateAll => Processor::rebate_all(invoke_context),
    }
}

pub struct Processor;

impl Processor {
    fn add_or_update_fees(
        invoke_context: &mut InvokeContext,
        fees: u64,
    ) -> Result<(), InstructionError> {
        let transaction_context = &invoke_context.transaction_context;
        let instruction_context = transaction_context.get_current_instruction_context()?;

        let owner = instruction_context.try_borrow_instruction_account(transaction_context, 0)?;

        if !owner.is_signer() {
            ic_msg!(invoke_context, "Authority account must be a signer");
            return Err(InstructionError::MissingRequiredSignature);
        }

        let writable_account = {
            let index_in_transaction =
                instruction_context.get_index_of_instruction_account_in_transaction(1)?;
            let writable_account_key =
                transaction_context.get_key_of_account_at_index(index_in_transaction)?;
            if writable_account_key.eq(owner.get_key()) {
                owner
            } else {
                let writable_account =
                    instruction_context.try_borrow_instruction_account(transaction_context, 1)?;

                if !writable_account.get_owner().eq(owner.get_key()) {
                    ic_msg!(
                        invoke_context,
                        "Invalid account owner {} instead of {}",
                        writable_account.get_owner().to_string(),
                        owner.get_key().to_string()
                    );
                    return Err(InstructionError::IllegalOwner);
                }

                drop(owner);
                writable_account
            }
        };

        if writable_account.get_rent_epoch() != 0 {
            return Err(InstructionError::CannotSetAppFeesForAccountWithRentEpoch);
        }

        let writable_account_key = *writable_account.get_key();
        ic_msg!(
            invoke_context,
            "ApplicationFeesInstuctions::Update called for {} to change fees to {}",
            writable_account_key.to_string(),
            fees
        );
        drop(writable_account);

        invoke_context
            .application_fee_changes
            .updated
            .push((writable_account_key, fees));
        Ok(())
    }

    fn rebate(
        invoke_context: &mut InvokeContext,
        rebate_fees: u64,
    ) -> Result<(), InstructionError> {
        let transaction_context = &invoke_context.transaction_context;
        let instruction_context = transaction_context.get_current_instruction_context()?;

        let owner = instruction_context.try_borrow_instruction_account(transaction_context, 0)?;
        if !owner.is_signer() {
            ic_msg!(invoke_context, "Authority account must be a signer");
            return Err(InstructionError::MissingRequiredSignature);
        }

        let index_in_transaction =
            instruction_context.get_index_of_instruction_account_in_transaction(1)?;
        let writable_account_key =
            transaction_context.get_key_of_account_at_index(index_in_transaction)?;
        let writable_account = {
            if writable_account_key.eq(owner.get_key()) {
                owner
            } else {
                let writable_account =
                    instruction_context.try_borrow_instruction_account(transaction_context, 1)?;

                if !writable_account.get_owner().eq(owner.get_key()) {
                    ic_msg!(
                        invoke_context,
                        "Invalid account owner {} instead of {}",
                        writable_account.get_owner().to_string(),
                        owner.get_key().to_string()
                    );
                    return Err(InstructionError::IllegalOwner);
                }

                drop(owner);
                writable_account
            }
        };
        drop(writable_account);
        // do rebate / update the application fees and add the rest into rebate
        let lamports_rebated = {
            let lamports = invoke_context
                .application_fee_changes
                .application_fees
                .get_mut(&writable_account_key);
            if let Some(lamports) = lamports {
                let lamports_rebated = min(*lamports, rebate_fees);
                // update app fees
                *lamports = lamports.saturating_sub(lamports_rebated);
                lamports_rebated
            } else {
                0
            }
        };
        // log message
        if lamports_rebated > 0 {
            invoke_context
                .application_fee_changes
                .rebated
                .insert(*writable_account_key, lamports_rebated);
            ic_msg!(
                invoke_context,
                "application fees rebated for writable account {} lamports {}",
                writable_account_key.to_string(),
                lamports_rebated
            );
        }
        Ok(())
    }

    fn rebate_all(invoke_context: &mut InvokeContext) -> Result<(), InstructionError> {
        let transaction_context = &invoke_context.transaction_context;
        let instruction_context = transaction_context.get_current_instruction_context()?;
        let owner = instruction_context.try_borrow_instruction_account(transaction_context, 0)?;
        let owner_key = *owner.get_key();
        if !owner.is_signer() {
            ic_msg!(invoke_context, "Authority account must be a signer");
            return Err(InstructionError::MissingRequiredSignature);
        }
        drop(owner);

        let number_of_accounts = transaction_context.get_number_of_accounts();
        for i in 0..number_of_accounts {
            let account = transaction_context.get_account_at_index(i)?;
            let key = transaction_context.get_key_of_account_at_index(i)?;
            let borrowed_account = account.try_borrow();
            if let Ok(borrowed_account) = borrowed_account {
                let account_owner = borrowed_account.owner();
                if owner_key.eq(account_owner) && borrowed_account.has_application_fees() {
                    let lamports_rebated = invoke_context
                        .application_fee_changes
                        .application_fees
                        .get_mut(key);
                    // we have to do this because we have already borrowed invoke context as a mut
                    let mut rebated_amount = None;
                    if let Some(lamports_rebated) = lamports_rebated {
                        rebated_amount = Some(*lamports_rebated);
                        // add these in rabates map
                        invoke_context
                            .application_fee_changes
                            .rebated
                            .insert(*key, *lamports_rebated);
                        // update the application fee charged to 0
                        *lamports_rebated = 0;
                    }

                    // log message
                    if let Some(rebated_amount) = rebated_amount {
                        ic_msg!(
                            invoke_context,
                            "application fees rebated for writable account {} lamports {}",
                            key.to_string(),
                            rebated_amount,
                        );
                    }
                }
            }
        }
        Ok(())
    }
}
