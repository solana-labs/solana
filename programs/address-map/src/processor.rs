use {
    crate::{
        instruction::AddressMapInstruction,
        state::{AddressMap, AddressMapState, ADDRESS_MAP_ENTRIES_START},
        DEACTIVATION_COOLDOWN,
    },
    solana_program_runtime::{
        instruction_processor::InstructionProcessor,
        invoke_context::{get_sysvar, InvokeContext},
    },
    solana_sdk::{
        account::WritableAccount,
        account_utils::State,
        clock::Slot,
        instruction::InstructionError,
        keyed_account::keyed_account_at_index,
        program_utils::limited_deserialize,
        pubkey::{Pubkey, PUBKEY_BYTES},
        system_instruction,
        sysvar::{
            clock::{self, Clock},
            epoch_schedule::{self, EpochSchedule},
            rent::{self, Rent},
        },
    },
};

pub fn process_instruction(
    _first_instruction_account: usize,
    instruction_data: &[u8],
    invoke_context: &mut dyn InvokeContext,
) -> Result<(), InstructionError> {
    let keyed_accounts = invoke_context.get_keyed_accounts()?;

    match limited_deserialize(instruction_data)? {
        AddressMapInstruction::InitializeAccount {
            bump_seed,
            num_entries,
            authority,
        } => {
            const MAP_INDEX: usize = 0;
            const PAYER_INDEX: usize = 1;

            let map_account = keyed_account_at_index(keyed_accounts, MAP_INDEX)?;
            if map_account.data_len()? > 0 {
                return Err(InstructionError::AccountAlreadyInitialized);
            }

            let payer_account = keyed_account_at_index(keyed_accounts, PAYER_INDEX)?;
            let payer_key = if let Some(payer_key) = payer_account.signer_key() {
                *payer_key
            } else {
                return Err(InstructionError::MissingRequiredSignature);
            };

            let clock: Clock = get_sysvar(invoke_context, &clock::id())?;
            let current_epoch = clock.epoch;

            // Use a derived address to ensure that an address map can never be
            // initialized more than once at the same address.
            let map_key = *map_account.unsigned_key();
            if Ok(map_key)
                != Pubkey::create_program_address(
                    &[
                        payer_key.as_ref(),
                        &current_epoch.to_le_bytes(),
                        &[bump_seed],
                    ],
                    &crate::id(),
                )
            {
                return Err(InstructionError::InvalidArgument);
            }

            let signers = &[map_key, payer_key];
            let entries_size = usize::from(num_entries).saturating_mul(PUBKEY_BYTES);
            let map_account_len = ADDRESS_MAP_ENTRIES_START.saturating_add(entries_size);
            let rent: Rent = get_sysvar(invoke_context, &rent::id())?;
            let required_lamports = rent
                .minimum_balance(map_account_len)
                .max(1)
                .saturating_sub(map_account.lamports()?);

            if required_lamports > 0 {
                InstructionProcessor::native_invoke(
                    invoke_context,
                    system_instruction::transfer(&payer_key, &map_key, required_lamports),
                    signers,
                )?;
            }

            InstructionProcessor::native_invoke(
                invoke_context,
                system_instruction::allocate(&map_key, map_account_len as u64),
                signers,
            )?;

            InstructionProcessor::native_invoke(
                invoke_context,
                system_instruction::assign(&map_key, &crate::id()),
                signers,
            )?;

            let keyed_accounts = invoke_context.get_keyed_accounts()?;
            let map_account = keyed_account_at_index(keyed_accounts, MAP_INDEX)?;
            map_account.set_state(&AddressMapState::Initialized(AddressMap {
                authority: Some(authority),
                activation_slot: Slot::MAX,
                deactivation_slot: Slot::MAX,
                num_entries,
            }))?;
        }
        AddressMapInstruction::SetAuthority { new_authority } => {
            let map_account = keyed_account_at_index(keyed_accounts, 0)?;
            if map_account.owner()? != crate::id() {
                return Err(InstructionError::InvalidAccountOwner);
            }

            let authority_account = keyed_account_at_index(keyed_accounts, 1)?;
            let mut map = if let AddressMapState::Initialized(map) = map_account.state()? {
                if map.authority.is_none() {
                    return Err(InstructionError::Immutable);
                }
                if map.authority != Some(*authority_account.unsigned_key()) {
                    return Err(InstructionError::IncorrectAuthority);
                }
                if authority_account.signer_key().is_none() {
                    return Err(InstructionError::MissingRequiredSignature);
                }
                // Cannot make an unactivated account immutable
                if new_authority.is_none() && map.activation_slot == Slot::MAX {
                    return Err(InstructionError::InvalidInstructionData);
                }
                map
            } else {
                return Err(InstructionError::UninitializedAccount);
            };

            map.authority = new_authority;
            map_account.set_state(&AddressMapState::Initialized(map))?;
        }
        AddressMapInstruction::InsertEntries { offset, entries } => {
            let map_account = keyed_account_at_index(keyed_accounts, 0)?;
            if map_account.owner()? != crate::id() {
                return Err(InstructionError::InvalidAccountOwner);
            }

            let authority_account = keyed_account_at_index(keyed_accounts, 1)?;
            if let AddressMapState::Initialized(map) = map_account.state()? {
                if map.authority.is_none() {
                    return Err(InstructionError::Immutable);
                }
                if map.authority != Some(*authority_account.unsigned_key()) {
                    return Err(InstructionError::IncorrectAuthority);
                }
                if authority_account.signer_key().is_none() {
                    return Err(InstructionError::MissingRequiredSignature);
                }
                // cannot modify activated map accounts
                if map.activation_slot != Slot::MAX {
                    return Err(InstructionError::InvalidInstructionData);
                }
            } else {
                return Err(InstructionError::UninitializedAccount);
            }

            let insert_start = ADDRESS_MAP_ENTRIES_START.saturating_add(usize::from(offset));
            let insert_end =
                insert_start.saturating_add(PUBKEY_BYTES.saturating_mul(entries.len()));
            let mut map_account_ref = map_account.try_account_ref_mut()?;
            let serialized_map: &mut [u8] = &mut map_account_ref.data_as_mut_slice();
            if serialized_map.len() < insert_end {
                return Err(InstructionError::InvalidInstructionData);
            }

            let mut start = insert_start;
            for entry in entries {
                let end = start.saturating_add(PUBKEY_BYTES);
                serialized_map[start..end].copy_from_slice(entry.as_ref());
                start = end;
            }
        }
        AddressMapInstruction::Activate => {
            let map_account = keyed_account_at_index(keyed_accounts, 0)?;
            if map_account.owner()? != crate::id() {
                return Err(InstructionError::InvalidAccountOwner);
            }

            let authority_account = keyed_account_at_index(keyed_accounts, 1)?;
            let mut map = if let AddressMapState::Initialized(map) = map_account.state()? {
                if map.authority.is_none() {
                    return Err(InstructionError::Immutable);
                }
                if map.authority != Some(*authority_account.unsigned_key()) {
                    return Err(InstructionError::IncorrectAuthority);
                }
                if authority_account.signer_key().is_none() {
                    return Err(InstructionError::MissingRequiredSignature);
                }
                // Cannot activate an already activated account
                if map.activation_slot != Slot::MAX {
                    return Err(InstructionError::InvalidInstructionData);
                }
                map
            } else {
                return Err(InstructionError::UninitializedAccount);
            };

            let clock: Clock = get_sysvar(invoke_context, &clock::id())?;
            map.activation_slot = clock.slot;
            map_account.set_state(&AddressMapState::Initialized(map))?;
        }
        AddressMapInstruction::Deactivate => {
            let map_account = keyed_account_at_index(keyed_accounts, 0)?;
            if map_account.owner()? != crate::id() {
                return Err(InstructionError::InvalidAccountOwner);
            }

            let authority_account = keyed_account_at_index(keyed_accounts, 1)?;
            let mut map = if let AddressMapState::Initialized(map) = map_account.state()? {
                if map.authority.is_none() {
                    return Err(InstructionError::Immutable);
                }
                if map.authority != Some(*authority_account.unsigned_key()) {
                    return Err(InstructionError::IncorrectAuthority);
                }
                if authority_account.signer_key().is_none() {
                    return Err(InstructionError::MissingRequiredSignature);
                }
                // Cannot deactivate an unactivated account
                if map.activation_slot == Slot::MAX {
                    return Err(InstructionError::InvalidInstructionData);
                }
                // Cannot deactivate an already deactivated account
                if map.deactivation_slot != Slot::MAX {
                    return Err(InstructionError::InvalidInstructionData);
                }
                map
            } else {
                return Err(InstructionError::UninitializedAccount);
            };

            let clock: Clock = get_sysvar(invoke_context, &clock::id())?;
            map.deactivation_slot = clock.slot;
            map_account.set_state(&AddressMapState::Initialized(map))?;
        }
        AddressMapInstruction::CloseAccount => {
            let map_account = keyed_account_at_index(keyed_accounts, 0)?;
            if map_account.owner()? != crate::id() {
                return Err(InstructionError::InvalidAccountOwner);
            }

            let recipient_account = keyed_account_at_index(keyed_accounts, 1)?;
            if let Ok(AddressMapState::Initialized(map)) = map_account.state() {
                let authority_account = keyed_account_at_index(keyed_accounts, 2)?;
                if map.authority.is_none() {
                    return Err(InstructionError::Immutable);
                }
                if map.authority != Some(*authority_account.unsigned_key()) {
                    return Err(InstructionError::IncorrectAuthority);
                }
                if authority_account.signer_key().is_none() {
                    return Err(InstructionError::MissingRequiredSignature);
                }
                if map.activation_slot != Slot::MAX {
                    if map.deactivation_slot == Slot::MAX {
                        return Err(InstructionError::InvalidInstructionData);
                    }

                    let clock: Clock = get_sysvar(invoke_context, &clock::id())?;
                    let epoch_schedule: EpochSchedule =
                        get_sysvar(invoke_context, &epoch_schedule::id())?;
                    let current_epoch = clock.epoch;
                    let deactivation_epoch = epoch_schedule.get_epoch(map.deactivation_slot);
                    let first_inactive_epoch =
                        deactivation_epoch.saturating_add(DEACTIVATION_COOLDOWN);
                    if current_epoch < first_inactive_epoch {
                        return Err(InstructionError::InvalidInstructionData);
                    }
                }
                map_account.set_state(&AddressMapState::Uninitialized)?;
            }

            recipient_account
                .try_account_ref_mut()?
                .checked_add_lamports(map_account.lamports()?)?;
            map_account.try_account_ref_mut()?.set_lamports(0);
        }
    }

    Ok(())
}
