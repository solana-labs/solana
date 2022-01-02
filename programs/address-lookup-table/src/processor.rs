use {
    crate::{
        instruction::ProgramInstruction,
        state::{
            AddressLookupTable, LookupTableMeta, LookupTableStatus, ProgramState,
            LOOKUP_TABLE_MAX_ADDRESSES, LOOKUP_TABLE_META_SIZE,
        },
    },
    solana_program_runtime::{ic_msg, invoke_context::InvokeContext},
    solana_sdk::{
        account::{ReadableAccount, WritableAccount},
        account_utils::State,
        clock::Slot,
        instruction::InstructionError,
        keyed_account::keyed_account_at_index,
        program_utils::limited_deserialize,
        pubkey::{Pubkey, PUBKEY_BYTES},
        slot_hashes::SlotHashes,
        system_instruction,
        sysvar::{
            clock::{self, Clock},
            rent::{self, Rent},
            slot_hashes,
        },
    },
    std::convert::TryFrom,
};

pub fn process_instruction(
    first_instruction_account: usize,
    instruction_data: &[u8],
    invoke_context: &mut InvokeContext,
) -> Result<(), InstructionError> {
    match limited_deserialize(instruction_data)? {
        ProgramInstruction::CreateLookupTable {
            recent_slot,
            bump_seed,
        } => Processor::create_lookup_table(
            invoke_context,
            first_instruction_account,
            recent_slot,
            bump_seed,
        ),
        ProgramInstruction::FreezeLookupTable => {
            Processor::freeze_lookup_table(invoke_context, first_instruction_account)
        }
        ProgramInstruction::ExtendLookupTable { new_addresses } => {
            Processor::extend_lookup_table(invoke_context, first_instruction_account, new_addresses)
        }
        ProgramInstruction::DeactivateLookupTable => {
            Processor::deactivate_lookup_table(invoke_context, first_instruction_account)
        }
        ProgramInstruction::CloseLookupTable => {
            Processor::close_lookup_table(invoke_context, first_instruction_account)
        }
    }
}

fn checked_add(a: usize, b: usize) -> Result<usize, InstructionError> {
    a.checked_add(b).ok_or(InstructionError::ArithmeticOverflow)
}

pub struct Processor;
impl Processor {
    fn create_lookup_table(
        invoke_context: &mut InvokeContext,
        first_instruction_account: usize,
        untrusted_recent_slot: Slot,
        bump_seed: u8,
    ) -> Result<(), InstructionError> {
        let keyed_accounts = invoke_context.get_keyed_accounts()?;

        let lookup_table_account =
            keyed_account_at_index(keyed_accounts, first_instruction_account)?;
        if lookup_table_account.data_len()? > 0 {
            ic_msg!(invoke_context, "Table account must not be allocated");
            return Err(InstructionError::AccountAlreadyInitialized);
        }

        let authority_account =
            keyed_account_at_index(keyed_accounts, checked_add(first_instruction_account, 1)?)?;
        let authority_key = *authority_account.signer_key().ok_or_else(|| {
            ic_msg!(invoke_context, "Authority account must be a signer");
            InstructionError::MissingRequiredSignature
        })?;

        let payer_account =
            keyed_account_at_index(keyed_accounts, checked_add(first_instruction_account, 2)?)?;
        let payer_key = *payer_account.signer_key().ok_or_else(|| {
            ic_msg!(invoke_context, "Payer account must be a signer");
            InstructionError::MissingRequiredSignature
        })?;

        let derivation_slot = {
            let slot_hashes: SlotHashes = invoke_context.get_sysvar(&slot_hashes::id())?;
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
            &crate::id(),
        )?;

        let table_key = *lookup_table_account.unsigned_key();
        if table_key != derived_table_key {
            ic_msg!(
                invoke_context,
                "Table address must match derived address: {}",
                derived_table_key
            );
            return Err(InstructionError::InvalidArgument);
        }

        let table_account_data_len = LOOKUP_TABLE_META_SIZE;
        let rent: Rent = invoke_context.get_sysvar(&rent::id())?;
        let required_lamports = rent
            .minimum_balance(table_account_data_len)
            .max(1)
            .saturating_sub(lookup_table_account.lamports()?);

        if required_lamports > 0 {
            invoke_context.native_invoke(
                system_instruction::transfer(&payer_key, &table_key, required_lamports),
                &[payer_key],
            )?;
        }

        invoke_context.native_invoke(
            system_instruction::allocate(&table_key, table_account_data_len as u64),
            &[table_key],
        )?;

        invoke_context.native_invoke(
            system_instruction::assign(&table_key, &crate::id()),
            &[table_key],
        )?;

        let keyed_accounts = invoke_context.get_keyed_accounts()?;
        let lookup_table_account =
            keyed_account_at_index(keyed_accounts, first_instruction_account)?;
        lookup_table_account.set_state(&ProgramState::LookupTable(LookupTableMeta::new(
            authority_key,
        )))?;

        Ok(())
    }

    fn freeze_lookup_table(
        invoke_context: &mut InvokeContext,
        first_instruction_account: usize,
    ) -> Result<(), InstructionError> {
        let keyed_accounts = invoke_context.get_keyed_accounts()?;

        let lookup_table_account =
            keyed_account_at_index(keyed_accounts, first_instruction_account)?;
        if lookup_table_account.owner()? != crate::id() {
            return Err(InstructionError::InvalidAccountOwner);
        }

        let authority_account =
            keyed_account_at_index(keyed_accounts, checked_add(first_instruction_account, 1)?)?;
        if authority_account.signer_key().is_none() {
            return Err(InstructionError::MissingRequiredSignature);
        }

        let lookup_table_account_ref = lookup_table_account.try_account_ref()?;
        let lookup_table_data = lookup_table_account_ref.data();
        let lookup_table = AddressLookupTable::deserialize(lookup_table_data)?;

        if lookup_table.meta.authority.is_none() {
            ic_msg!(invoke_context, "Lookup table is already frozen");
            return Err(InstructionError::Immutable);
        }
        if lookup_table.meta.authority != Some(*authority_account.unsigned_key()) {
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
        drop(lookup_table_account_ref);

        lookup_table_meta.authority = None;
        AddressLookupTable::overwrite_meta_data(
            lookup_table_account
                .try_account_ref_mut()?
                .data_as_mut_slice(),
            lookup_table_meta,
        )?;

        Ok(())
    }

    fn extend_lookup_table(
        invoke_context: &mut InvokeContext,
        first_instruction_account: usize,
        new_addresses: Vec<Pubkey>,
    ) -> Result<(), InstructionError> {
        let keyed_accounts = invoke_context.get_keyed_accounts()?;

        let lookup_table_account =
            keyed_account_at_index(keyed_accounts, first_instruction_account)?;
        if lookup_table_account.owner()? != crate::id() {
            return Err(InstructionError::InvalidAccountOwner);
        }

        let authority_account =
            keyed_account_at_index(keyed_accounts, checked_add(first_instruction_account, 1)?)?;
        if authority_account.signer_key().is_none() {
            return Err(InstructionError::MissingRequiredSignature);
        }

        let payer_account =
            keyed_account_at_index(keyed_accounts, checked_add(first_instruction_account, 2)?)?;
        let payer_key = if let Some(payer_key) = payer_account.signer_key() {
            *payer_key
        } else {
            ic_msg!(invoke_context, "Payer account must be a signer");
            return Err(InstructionError::MissingRequiredSignature);
        };

        let lookup_table_account_ref = lookup_table_account.try_account_ref()?;
        let lookup_table_data = lookup_table_account_ref.data();
        let mut lookup_table = AddressLookupTable::deserialize(lookup_table_data)?;

        if lookup_table.meta.authority.is_none() {
            return Err(InstructionError::Immutable);
        }
        if lookup_table.meta.authority != Some(*authority_account.unsigned_key()) {
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

        let clock: Clock = invoke_context.get_sysvar(&clock::id())?;
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
        drop(lookup_table_account_ref);

        let new_table_data_len = checked_add(
            LOOKUP_TABLE_META_SIZE,
            new_table_addresses_len.saturating_mul(PUBKEY_BYTES),
        )?;

        {
            let mut lookup_table_account_ref_mut = lookup_table_account.try_account_ref_mut()?;
            AddressLookupTable::overwrite_meta_data(
                lookup_table_account_ref_mut.data_as_mut_slice(),
                lookup_table_meta,
            )?;

            let table_data = lookup_table_account_ref_mut.data_mut();
            for new_address in new_addresses {
                table_data.extend_from_slice(new_address.as_ref());
            }
        }

        let rent: Rent = invoke_context.get_sysvar(&rent::id())?;
        let required_lamports = rent
            .minimum_balance(new_table_data_len)
            .max(1)
            .saturating_sub(lookup_table_account.lamports()?);

        let table_key = *lookup_table_account.unsigned_key();
        if required_lamports > 0 {
            invoke_context.native_invoke(
                system_instruction::transfer(&payer_key, &table_key, required_lamports),
                &[payer_key],
            )?;
        }

        Ok(())
    }

    fn deactivate_lookup_table(
        invoke_context: &mut InvokeContext,
        first_instruction_account: usize,
    ) -> Result<(), InstructionError> {
        let keyed_accounts = invoke_context.get_keyed_accounts()?;

        let lookup_table_account =
            keyed_account_at_index(keyed_accounts, first_instruction_account)?;
        if lookup_table_account.owner()? != crate::id() {
            return Err(InstructionError::InvalidAccountOwner);
        }

        let authority_account =
            keyed_account_at_index(keyed_accounts, checked_add(first_instruction_account, 1)?)?;
        if authority_account.signer_key().is_none() {
            return Err(InstructionError::MissingRequiredSignature);
        }

        let lookup_table_account_ref = lookup_table_account.try_account_ref()?;
        let lookup_table_data = lookup_table_account_ref.data();
        let lookup_table = AddressLookupTable::deserialize(lookup_table_data)?;

        if lookup_table.meta.authority.is_none() {
            ic_msg!(invoke_context, "Lookup table is frozen");
            return Err(InstructionError::Immutable);
        }
        if lookup_table.meta.authority != Some(*authority_account.unsigned_key()) {
            return Err(InstructionError::IncorrectAuthority);
        }
        if lookup_table.meta.deactivation_slot != Slot::MAX {
            ic_msg!(invoke_context, "Lookup table is already deactivated");
            return Err(InstructionError::InvalidArgument);
        }

        let mut lookup_table_meta = lookup_table.meta;
        drop(lookup_table_account_ref);

        let clock: Clock = invoke_context.get_sysvar(&clock::id())?;
        lookup_table_meta.deactivation_slot = clock.slot;

        AddressLookupTable::overwrite_meta_data(
            lookup_table_account
                .try_account_ref_mut()?
                .data_as_mut_slice(),
            lookup_table_meta,
        )?;

        Ok(())
    }

    fn close_lookup_table(
        invoke_context: &mut InvokeContext,
        first_instruction_account: usize,
    ) -> Result<(), InstructionError> {
        let keyed_accounts = invoke_context.get_keyed_accounts()?;

        let lookup_table_account =
            keyed_account_at_index(keyed_accounts, first_instruction_account)?;
        if lookup_table_account.owner()? != crate::id() {
            return Err(InstructionError::InvalidAccountOwner);
        }

        let authority_account =
            keyed_account_at_index(keyed_accounts, checked_add(first_instruction_account, 1)?)?;
        if authority_account.signer_key().is_none() {
            return Err(InstructionError::MissingRequiredSignature);
        }

        let recipient_account =
            keyed_account_at_index(keyed_accounts, checked_add(first_instruction_account, 2)?)?;
        if recipient_account.unsigned_key() == lookup_table_account.unsigned_key() {
            ic_msg!(
                invoke_context,
                "Lookup table cannot be the recipient of reclaimed lamports"
            );
            return Err(InstructionError::InvalidArgument);
        }

        let lookup_table_account_ref = lookup_table_account.try_account_ref()?;
        let lookup_table_data = lookup_table_account_ref.data();
        let lookup_table = AddressLookupTable::deserialize(lookup_table_data)?;

        if lookup_table.meta.authority.is_none() {
            ic_msg!(invoke_context, "Lookup table is frozen");
            return Err(InstructionError::Immutable);
        }
        if lookup_table.meta.authority != Some(*authority_account.unsigned_key()) {
            return Err(InstructionError::IncorrectAuthority);
        }

        let clock: Clock = invoke_context.get_sysvar(&clock::id())?;
        let slot_hashes: SlotHashes = invoke_context.get_sysvar(&slot_hashes::id())?;

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

        drop(lookup_table_account_ref);

        let withdrawn_lamports = lookup_table_account.lamports()?;
        recipient_account
            .try_account_ref_mut()?
            .checked_add_lamports(withdrawn_lamports)?;

        let mut lookup_table_account = lookup_table_account.try_account_ref_mut()?;
        lookup_table_account.set_data(Vec::new());
        lookup_table_account.set_lamports(0);

        Ok(())
    }
}
