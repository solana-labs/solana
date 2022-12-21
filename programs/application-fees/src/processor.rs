use solana_program::{pubkey::Pubkey, system_instruction};
use solana_program_runtime::{invoke_context::InvokeContext, ic_msg};
use solana_sdk::{instruction::InstructionError, transaction_context::IndexOfAccount, program_utils::limited_deserialize, application_fees::{ApplicationFeesInstuctions, self, APPLICATION_FEE_STRUCTURE_SIZE, ApplicationFeeStructure}, feature_set};

pub fn process_instruction(
    _first_instruction_account: IndexOfAccount,
    invoke_context: &mut InvokeContext,
) -> Result<(), InstructionError> {
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;
    let instruction_data = instruction_context.get_instruction_data();

    match limited_deserialize(instruction_data)? {
        ApplicationFeesInstuctions::AddOrUpdateFee { fees } => {
            Processor::add_or_update_fees(invoke_context, fees)
        },
        ApplicationFeesInstuctions::RemoveFees => {
            Processor::remove_fees(invoke_context)
        },
        ApplicationFeesInstuctions::Rebate => {
            Ok(())
        },
        ApplicationFeesInstuctions::RebateAll => {
            Ok(())
        }
    }
}

pub struct Processor;

impl Processor {
    fn add_or_update_fees (invoke_context: &mut InvokeContext, fees : u64) -> Result<(), InstructionError> {
        let transaction_context = &invoke_context.transaction_context;
        let instruction_context = transaction_context.get_current_instruction_context()?;

        let writable_account = instruction_context.try_borrow_instruction_account(transaction_context, 0)?;
        let owner = instruction_context.try_borrow_instruction_account(transaction_context, 1)?;
        let pda = instruction_context.try_borrow_instruction_account(transaction_context, 2)?;

        if !owner.is_signer() {
            ic_msg!(invoke_context, "Authority account must be a signer");
            return Err(InstructionError::MissingRequiredSignature);
        }

        if !writable_account.get_owner().eq(owner.get_key()) {
            ic_msg!(invoke_context, "Invalid account owner");
            return Err(InstructionError::IllegalOwner);
        }
        drop(owner);

        let (calculated_pda, _bump) = Pubkey::find_program_address(&[&writable_account.get_key().to_bytes()], &crate::id());
        if !calculated_pda.eq(pda.get_key()) {
            ic_msg!(invoke_context, "Invalid pda to store fee info");
            return Err(InstructionError::InvalidArgument);
        }
        drop(writable_account);

        let pda_key = *pda.get_key();
        let is_pda_empty = pda.get_data().is_empty();
        let pda_lamports = pda.get_lamports();
        drop(pda);
        // allocate pda to store application fee strucutre
        if is_pda_empty {
            let payer = instruction_context.try_borrow_instruction_account(transaction_context, 3)?;
            let payer_key = *payer.get_key();
            if !payer.is_signer() {
                ic_msg!(invoke_context, "Payer account must be a signer");
                return Err(InstructionError::MissingRequiredSignature);
            }
            drop(payer);

            let account_data_len = APPLICATION_FEE_STRUCTURE_SIZE;
            let rent = invoke_context.get_sysvar_cache().get_rent()?;
            let required_lamports = rent
                .minimum_balance(account_data_len)
                .max(1)
                .saturating_sub(pda_lamports);
                
            if required_lamports > 0 {
                invoke_context.native_invoke(
                    system_instruction::transfer(&payer_key, &pda_key, required_lamports),
                    &[payer_key],
                )?;
            }
    
            invoke_context.native_invoke(
                system_instruction::allocate(&pda_key, account_data_len as u64),
                &[pda_key],
            )?;

            invoke_context.native_invoke(
                system_instruction::assign(&pda_key, &crate::id()),
                &[pda_key],
            )?;
        }

        let transaction_context = &invoke_context.transaction_context;
        let instruction_context = transaction_context.get_current_instruction_context()?;
        let mut pda_account =
            instruction_context.try_borrow_instruction_account(transaction_context, 2)?;
        
        let application_fee_structure = ApplicationFeeStructure {
            fee_lamports : fees,
            version : 1,
            _padding : [0;8],
        };
        let seralized = bincode::serialize(&application_fee_structure).unwrap();
        pda_account.set_data(seralized)?;

        Ok(())
    } 

    fn remove_fees( invoke_context: &mut InvokeContext) -> Result<(), InstructionError> {
        let transaction_context = &invoke_context.transaction_context;
        let instruction_context = transaction_context.get_current_instruction_context()?;

        let writable_account = instruction_context.try_borrow_instruction_account(transaction_context, 0)?;
        let owner = instruction_context.try_borrow_instruction_account(transaction_context, 1)?;
        let pda = instruction_context.try_borrow_instruction_account(transaction_context, 2)?;

        if !owner.is_signer() {
            ic_msg!(invoke_context, "Authority account must be a signer");
            return Err(InstructionError::MissingRequiredSignature);
        }

        if !writable_account.get_owner().eq(owner.get_key()) {
            ic_msg!(invoke_context, "Invalid account owner");
            return Err(InstructionError::IllegalOwner);
        }
        
        let (calculated_pda, _bump) = Pubkey::find_program_address(&[&writable_account.get_key().to_bytes()], &crate::id());
        if !calculated_pda.eq(pda.get_key()) {
            ic_msg!(invoke_context, "Invalid pda to store fee info");
            return Err(InstructionError::InvalidArgument);
        }

        if pda.get_data().is_empty() {
            ic_msg!(invoke_context, "No fees structure associated with the writable account");
            return Err(InstructionError::InvalidArgument);
        }
        let withdrawn_lamports = pda.get_lamports();
        drop(writable_account);
        drop(owner);
        drop(pda);

        let mut writable_account =
            instruction_context.try_borrow_instruction_account(transaction_context, 0)?;
        writable_account.checked_add_lamports(withdrawn_lamports)?;
        drop(writable_account);

        let mut pda = instruction_context.try_borrow_instruction_account(transaction_context, 2)?;
        pda.set_data_length(0)?;
        pda.set_lamports(0)?;

        Ok(())
    }
}
