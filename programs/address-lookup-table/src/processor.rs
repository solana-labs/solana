use {
    solana_program_runtime::{declare_process_instruction, ic_msg, invoke_context::InvokeContext},
    solana_sdk::{
        address_lookup_table::{
            instruction::ProgramInstruction,
            program::{check_id, id},
            state::{
                AddressLookupTable, LookupTableMeta, LookupTableStatus, ProgramState,
                LOOKUP_TABLE_MAX_ADDRESSES, LOOKUP_TABLE_META_SIZE,
            },
        },
        clock::Slot,
        feature_set,
        instruction::InstructionError,
        program_utils::limited_deserialize,
        pubkey::{Pubkey, PUBKEY_BYTES},
        system_instruction,
    },
    std::convert::TryFrom,
};

pub const DEFAULT_COMPUTE_UNITS: u64 = 750;

declare_process_instruction!(Entrypoint, DEFAULT_COMPUTE_UNITS, |invoke_context| {
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;
    let instruction_data = instruction_context.get_instruction_data();
    match limited_deserialize(instruction_data)? {
        ProgramInstruction::CreateLookupTable {
            recent_slot,
            bump_seed,
        } => Processor::create_lookup_table(invoke_context, recent_slot, bump_seed),
        ProgramInstruction::FreezeLookupTable => Processor::freeze_lookup_table(invoke_context),
        ProgramInstruction::ExtendLookupTable { new_addresses } => {
            Processor::extend_lookup_table(invoke_context, new_addresses)
        }
        ProgramInstruction::DeactivateLookupTable => {
            Processor::deactivate_lookup_table(invoke_context)
        }
        ProgramInstruction::CloseLookupTable => Processor::close_lookup_table(invoke_context),
    }
});

fn checked_add(a: usize, b: usize) -> Result<usize, InstructionError> {
    a.checked_add(b).ok_or(InstructionError::ArithmeticOverflow)
}

pub struct Processor;
impl Processor {
    fn create_lookup_table(
        invoke_context: &mut InvokeContext,
        untrusted_recent_slot: Slot,
        bump_seed: u8,
    ) -> Result<(), InstructionError> {
        let transaction_context = &invoke_context.transaction_context;
        let instruction_context = transaction_context.get_current_instruction_context()?;

        let lookup_table_account =
            instruction_context.try_borrow_instruction_account(transaction_context, 0)?;
        let lookup_table_lamports = lookup_table_account.get_lamports();
        let table_key = *lookup_table_account.get_key();
        let lookup_table_owner = *lookup_table_account.get_owner();
        if !invoke_context
            .feature_set
            .is_active(&feature_set::relax_authority_signer_check_for_lookup_table_creation::id())
            && !lookup_table_account.get_data().is_empty()
        {
            ic_msg!(invoke_context, "Table account must not be allocated");
            return Err(InstructionError::AccountAlreadyInitialized);
        }
        drop(lookup_table_account);

        let authority_account =
            instruction_context.try_borrow_instruction_account(transaction_context, 1)?;
        let authority_key = *authority_account.get_key();
        if !invoke_context
            .feature_set
            .is_active(&feature_set::relax_authority_signer_check_for_lookup_table_creation::id())
            && !authority_account.is_signer()
        {
            ic_msg!(invoke_context, "Authority account must be a signer");
            return Err(InstructionError::MissingRequiredSignature);
        }
        drop(authority_account);

        let payer_account =
            instruction_context.try_borrow_instruction_account(transaction_context, 2)?;
        let payer_key = *payer_account.get_key();
        if !payer_account.is_signer() {
            ic_msg!(invoke_context, "Payer account must be a signer");
            return Err(InstructionError::MissingRequiredSignature);
        }
        drop(payer_account);

        let derivation_slot = {
            let slot_hashes = invoke_context.get_sysvar_cache().get_slot_hashes()?;
            if slot_hashes.get(&untrusted_recent_slot).is_some() {
                Ok(untrusted_recent_slot)
            } else {
                ic_msg!(
                    invoke_context,
                    "{} is not a recent slot",
                    untrusted_recent_slot
                );
                Err(InstructionError::InvalidInstructionData)
            }
        }?;

        // Use a derived address to ensure that an address table can never be
        // initialized more than once at the same address.
        let derived_table_key = Pubkey::create_program_address(
            &[
                authority_key.as_ref(),
                &derivation_slot.to_le_bytes(),
                &[bump_seed],
            ],
            &id(),
        )?;

        if table_key != derived_table_key {
            ic_msg!(
                invoke_context,
                "Table address must match derived address: {}",
                derived_table_key
            );
            return Err(InstructionError::InvalidArgument);
        }

        if invoke_context
            .feature_set
            .is_active(&feature_set::relax_authority_signer_check_for_lookup_table_creation::id())
            && check_id(&lookup_table_owner)
        {
            return Ok(());
        }

        let table_account_data_len = LOOKUP_TABLE_META_SIZE;
        let rent = invoke_context.get_sysvar_cache().get_rent()?;
        let required_lamports = rent
            .minimum_balance(table_account_data_len)
            .max(1)
            .saturating_sub(lookup_table_lamports);

        if required_lamports > 0 {
            invoke_context.native_invoke(
                system_instruction::transfer(&payer_key, &table_key, required_lamports).into(),
                &[payer_key],
            )?;
        }

        invoke_context.native_invoke(
            system_instruction::allocate(&table_key, table_account_data_len as u64).into(),
            &[table_key],
        )?;

        invoke_context.native_invoke(
            system_instruction::assign(&table_key, &id()).into(),
            &[table_key],
        )?;

        let transaction_context = &invoke_context.transaction_context;
        let instruction_context = transaction_context.get_current_instruction_context()?;
        let mut lookup_table_account =
            instruction_context.try_borrow_instruction_account(transaction_context, 0)?;
        lookup_table_account.set_state(&ProgramState::LookupTable(LookupTableMeta::new(
            authority_key,
        )))?;

        Ok(())
    }

    fn freeze_lookup_table(invoke_context: &mut InvokeContext) -> Result<(), InstructionError> {
        let transaction_context = &invoke_context.transaction_context;
        let instruction_context = transaction_context.get_current_instruction_context()?;

        let lookup_table_account =
            instruction_context.try_borrow_instruction_account(transaction_context, 0)?;
        if *lookup_table_account.get_owner() != id() {
            return Err(InstructionError::InvalidAccountOwner);
        }
        drop(lookup_table_account);

        let authority_account =
            instruction_context.try_borrow_instruction_account(transaction_context, 1)?;
        let authority_key = *authority_account.get_key();
        if !authority_account.is_signer() {
            ic_msg!(invoke_context, "Authority account must be a signer");
            return Err(InstructionError::MissingRequiredSignature);
        }
        drop(authority_account);

        let mut lookup_table_account =
            instruction_context.try_borrow_instruction_account(transaction_context, 0)?;
        let lookup_table_data = lookup_table_account.get_data();
        let lookup_table = AddressLookupTable::deserialize(lookup_table_data)?;

        if lookup_table.meta.authority.is_none() {
            ic_msg!(invoke_context, "Lookup table is already frozen");
            return Err(InstructionError::Immutable);
        }
        if lookup_table.meta.authority != Some(authority_key) {
            return Err(InstructionError::IncorrectAuthority);
        }
        if lookup_table.meta.deactivation_slot != Slot::MAX {
            ic_msg!(invoke_context, "Deactivated tables cannot be frozen");
            return Err(InstructionError::InvalidArgument);
        }
        if lookup_table.addresses.is_empty() {
            ic_msg!(invoke_context, "Empty lookup tables cannot be frozen");
            return Err(InstructionError::InvalidInstructionData);
        }

        let mut lookup_table_meta = lookup_table.meta;
        lookup_table_meta.authority = None;
        AddressLookupTable::overwrite_meta_data(
            lookup_table_account.get_data_mut()?,
            lookup_table_meta,
        )?;

        Ok(())
    }

    fn extend_lookup_table(
        invoke_context: &mut InvokeContext,
        new_addresses: Vec<Pubkey>,
    ) -> Result<(), InstructionError> {
        let transaction_context = &invoke_context.transaction_context;
        let instruction_context = transaction_context.get_current_instruction_context()?;

        let lookup_table_account =
            instruction_context.try_borrow_instruction_account(transaction_context, 0)?;
        let table_key = *lookup_table_account.get_key();
        if *lookup_table_account.get_owner() != id() {
            return Err(InstructionError::InvalidAccountOwner);
        }
        drop(lookup_table_account);

        let authority_account =
            instruction_context.try_borrow_instruction_account(transaction_context, 1)?;
        let authority_key = *authority_account.get_key();
        if !authority_account.is_signer() {
            ic_msg!(invoke_context, "Authority account must be a signer");
            return Err(InstructionError::MissingRequiredSignature);
        }
        drop(authority_account);

        let mut lookup_table_account =
            instruction_context.try_borrow_instruction_account(transaction_context, 0)?;
        let lookup_table_data = lookup_table_account.get_data();
        let lookup_table_lamports = lookup_table_account.get_lamports();
        let mut lookup_table = AddressLookupTable::deserialize(lookup_table_data)?;

        if lookup_table.meta.authority.is_none() {
            return Err(InstructionError::Immutable);
        }
        if lookup_table.meta.authority != Some(authority_key) {
            return Err(InstructionError::IncorrectAuthority);
        }
        if lookup_table.meta.deactivation_slot != Slot::MAX {
            ic_msg!(invoke_context, "Deactivated tables cannot be extended");
            return Err(InstructionError::InvalidArgument);
        }
        if lookup_table.addresses.len() >= LOOKUP_TABLE_MAX_ADDRESSES {
            ic_msg!(
                invoke_context,
                "Lookup table is full and cannot contain more addresses"
            );
            return Err(InstructionError::InvalidArgument);
        }

        if new_addresses.is_empty() {
            ic_msg!(invoke_context, "Must extend with at least one address");
            return Err(InstructionError::InvalidInstructionData);
        }

        let new_table_addresses_len = lookup_table
            .addresses
            .len()
            .saturating_add(new_addresses.len());
        if new_table_addresses_len > LOOKUP_TABLE_MAX_ADDRESSES {
            ic_msg!(
                invoke_context,
                "Extended lookup table length {} would exceed max capacity of {}",
                new_table_addresses_len,
                LOOKUP_TABLE_MAX_ADDRESSES
            );
            return Err(InstructionError::InvalidInstructionData);
        }

        let clock = invoke_context.get_sysvar_cache().get_clock()?;
        if clock.slot != lookup_table.meta.last_extended_slot {
            lookup_table.meta.last_extended_slot = clock.slot;
            lookup_table.meta.last_extended_slot_start_index =
                u8::try_from(lookup_table.addresses.len()).map_err(|_| {
                    // This is impossible as long as the length of new_addresses
                    // is non-zero and LOOKUP_TABLE_MAX_ADDRESSES == u8::MAX + 1.
                    InstructionError::InvalidAccountData
                })?;
        }

        let lookup_table_meta = lookup_table.meta;
        let new_table_data_len = checked_add(
            LOOKUP_TABLE_META_SIZE,
            new_table_addresses_len.saturating_mul(PUBKEY_BYTES),
        )?;
        {
            AddressLookupTable::overwrite_meta_data(
                lookup_table_account.get_data_mut()?,
                lookup_table_meta,
            )?;
            for new_address in new_addresses {
                lookup_table_account.extend_from_slice(new_address.as_ref())?;
            }
        }
        drop(lookup_table_account);

        let rent = invoke_context.get_sysvar_cache().get_rent()?;
        let required_lamports = rent
            .minimum_balance(new_table_data_len)
            .max(1)
            .saturating_sub(lookup_table_lamports);

        if required_lamports > 0 {
            let payer_account =
                instruction_context.try_borrow_instruction_account(transaction_context, 2)?;
            let payer_key = *payer_account.get_key();
            if !payer_account.is_signer() {
                ic_msg!(invoke_context, "Payer account must be a signer");
                return Err(InstructionError::MissingRequiredSignature);
            }
            drop(payer_account);

            invoke_context.native_invoke(
                system_instruction::transfer(&payer_key, &table_key, required_lamports).into(),
                &[payer_key],
            )?;
        }

        Ok(())
    }

    fn deactivate_lookup_table(invoke_context: &mut InvokeContext) -> Result<(), InstructionError> {
        let transaction_context = &invoke_context.transaction_context;
        let instruction_context = transaction_context.get_current_instruction_context()?;

        let lookup_table_account =
            instruction_context.try_borrow_instruction_account(transaction_context, 0)?;
        if *lookup_table_account.get_owner() != id() {
            return Err(InstructionError::InvalidAccountOwner);
        }
        drop(lookup_table_account);

        let authority_account =
            instruction_context.try_borrow_instruction_account(transaction_context, 1)?;
        let authority_key = *authority_account.get_key();
        if !authority_account.is_signer() {
            ic_msg!(invoke_context, "Authority account must be a signer");
            return Err(InstructionError::MissingRequiredSignature);
        }
        drop(authority_account);

        let mut lookup_table_account =
            instruction_context.try_borrow_instruction_account(transaction_context, 0)?;
        let lookup_table_data = lookup_table_account.get_data();
        let lookup_table = AddressLookupTable::deserialize(lookup_table_data)?;

        if lookup_table.meta.authority.is_none() {
            ic_msg!(invoke_context, "Lookup table is frozen");
            return Err(InstructionError::Immutable);
        }
        if lookup_table.meta.authority != Some(authority_key) {
            return Err(InstructionError::IncorrectAuthority);
        }
        if lookup_table.meta.deactivation_slot != Slot::MAX {
            ic_msg!(invoke_context, "Lookup table is already deactivated");
            return Err(InstructionError::InvalidArgument);
        }

        let mut lookup_table_meta = lookup_table.meta;
        let clock = invoke_context.get_sysvar_cache().get_clock()?;
        lookup_table_meta.deactivation_slot = clock.slot;

        AddressLookupTable::overwrite_meta_data(
            lookup_table_account.get_data_mut()?,
            lookup_table_meta,
        )?;

        Ok(())
    }

    fn close_lookup_table(invoke_context: &mut InvokeContext) -> Result<(), InstructionError> {
        let transaction_context = &invoke_context.transaction_context;
        let instruction_context = transaction_context.get_current_instruction_context()?;

        let lookup_table_account =
            instruction_context.try_borrow_instruction_account(transaction_context, 0)?;
        if *lookup_table_account.get_owner() != id() {
            return Err(InstructionError::InvalidAccountOwner);
        }
        drop(lookup_table_account);

        let authority_account =
            instruction_context.try_borrow_instruction_account(transaction_context, 1)?;
        let authority_key = *authority_account.get_key();
        if !authority_account.is_signer() {
            ic_msg!(invoke_context, "Authority account must be a signer");
            return Err(InstructionError::MissingRequiredSignature);
        }
        drop(authority_account);

        instruction_context.check_number_of_instruction_accounts(3)?;
        if instruction_context.get_index_of_instruction_account_in_transaction(0)?
            == instruction_context.get_index_of_instruction_account_in_transaction(2)?
        {
            ic_msg!(
                invoke_context,
                "Lookup table cannot be the recipient of reclaimed lamports"
            );
            return Err(InstructionError::InvalidArgument);
        }

        let lookup_table_account =
            instruction_context.try_borrow_instruction_account(transaction_context, 0)?;
        let withdrawn_lamports = lookup_table_account.get_lamports();
        let lookup_table_data = lookup_table_account.get_data();
        let lookup_table = AddressLookupTable::deserialize(lookup_table_data)?;

        if lookup_table.meta.authority.is_none() {
            ic_msg!(invoke_context, "Lookup table is frozen");
            return Err(InstructionError::Immutable);
        }
        if lookup_table.meta.authority != Some(authority_key) {
            return Err(InstructionError::IncorrectAuthority);
        }

        let sysvar_cache = invoke_context.get_sysvar_cache();
        let clock = sysvar_cache.get_clock()?;
        let slot_hashes = sysvar_cache.get_slot_hashes()?;

        match lookup_table.meta.status(clock.slot, &slot_hashes) {
            LookupTableStatus::Activated => {
                ic_msg!(invoke_context, "Lookup table is not deactivated");
                Err(InstructionError::InvalidArgument)
            }
            LookupTableStatus::Deactivating { remaining_blocks } => {
                ic_msg!(
                    invoke_context,
                    "Table cannot be closed until it's fully deactivated in {} blocks",
                    remaining_blocks
                );
                Err(InstructionError::InvalidArgument)
            }
            LookupTableStatus::Deactivated => Ok(()),
        }?;
        drop(lookup_table_account);

        let mut recipient_account =
            instruction_context.try_borrow_instruction_account(transaction_context, 2)?;
        recipient_account.checked_add_lamports(withdrawn_lamports)?;
        drop(recipient_account);

        let mut lookup_table_account =
            instruction_context.try_borrow_instruction_account(transaction_context, 0)?;
        lookup_table_account.set_data_length(0)?;
        lookup_table_account.set_lamports(0)?;

        Ok(())
    }
}
