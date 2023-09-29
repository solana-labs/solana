use {
    super::Bank,
    log::*,
    solana_accounts_db::accounts_index::ZeroLamport,
    solana_sdk::{
        account::{Account, AccountSharedData, ReadableAccount},
        bpf_loader_upgradeable::{self, UpgradeableLoaderState},
        pubkey::Pubkey,
    },
    std::sync::atomic::Ordering::Relaxed,
};

/// Moves one account in place of another
/// `source`: the account to replace with
/// `destination`: the account to be replaced
fn move_account<U, V>(
    bank: &mut Bank,
    source_address: &Pubkey,
    source_account: &V,
    destination_address: &Pubkey,
    destination_account: Option<&U>,
) where
    U: ReadableAccount + Sync + ZeroLamport,
    V: ReadableAccount + Sync + ZeroLamport,
{
    let (destination_lamports, destination_len) = match destination_account {
        Some(destination_account) => (
            destination_account.lamports(),
            destination_account.data().len(),
        ),
        None => (0, 0),
    };

    // Burn lamports in the destination account
    bank.capitalization.fetch_sub(destination_lamports, Relaxed);

    // Transfer source account to destination account
    bank.store_account(destination_address, source_account);

    // Clear source account
    bank.store_account(source_address, &AccountSharedData::default());

    bank.calculate_and_update_accounts_data_size_delta_off_chain(
        destination_len,
        source_account.data().len(),
    );
}

/// Use to replace non-upgradeable programs by feature activation
/// `source`: the non-upgradeable program account to replace with
/// `destination`: the non-upgradeable program account to be replaced
#[allow(dead_code)]
pub(crate) fn replace_non_upgradeable_program_account(
    bank: &mut Bank,
    source_address: &Pubkey,
    destination_address: &Pubkey,
    datapoint_name: &'static str,
) {
    if let Some(destination_account) = bank.get_account_with_fixed_root(destination_address) {
        if let Some(source_account) = bank.get_account_with_fixed_root(source_address) {
            datapoint_info!(datapoint_name, ("slot", bank.slot, i64));

            move_account(
                bank,
                source_address,
                &source_account,
                destination_address,
                Some(&destination_account),
            );

            // Unload a program from the bank's cache
            bank.loaded_programs_cache
                .write()
                .unwrap()
                .remove_programs([*destination_address].into_iter());
        } else {
            warn!(
                "Unable to find source program {}. Destination: {}",
                source_address, destination_address
            );
            datapoint_warn!(datapoint_name, ("slot", bank.slot(), i64),);
        }
    } else {
        warn!("Unable to find destination program {}", destination_address);
        datapoint_warn!(datapoint_name, ("slot", bank.slot(), i64),);
    }
}

/// Use to replace an empty account with a program by feature activation
/// Note: The upgradeable program should have both:
///     - Program account
///     - Program data account
/// `source`: the upgradeable program account to replace with
/// `destination`: the empty account to be replaced
pub(crate) fn replace_empty_account_with_upgradeable_program(
    bank: &mut Bank,
    source_address: &Pubkey,
    destination_address: &Pubkey,
    datapoint_name: &'static str,
) {
    // Must be attempting to replace an empty account with a program
    // account _and_ data account
    if let Some(source_account) = bank.get_account_with_fixed_root(source_address) {
        let (destination_data_address, _) = Pubkey::find_program_address(
            &[destination_address.as_ref()],
            &bpf_loader_upgradeable::id(),
        );
        let (source_data_address, _) =
            Pubkey::find_program_address(&[source_address.as_ref()], &bpf_loader_upgradeable::id());

        // Make sure the data within the source account is the PDA of its
        // data account. This also means it has at least the necessary
        // lamports for rent.
        if let Ok(source_state) =
            bincode::deserialize::<UpgradeableLoaderState>(source_account.data())
        {
            match source_state {
                UpgradeableLoaderState::Program {
                    programdata_address: _,
                } => {
                    if let Some(source_data_account) =
                        bank.get_account_with_fixed_root(&source_data_address)
                    {
                        // Make sure the destination account is empty
                        // We aren't going to check that there isn't a data account at
                        // the known program-derived address (ie. `destination_data_address`),
                        // because if it exists, it will be overwritten
                        if bank
                            .get_account_with_fixed_root(destination_address)
                            .is_none()
                        {
                            let state = UpgradeableLoaderState::Program {
                                programdata_address: destination_data_address,
                            };
                            if let Ok(data) = bincode::serialize(&state) {
                                let lamports =
                                    bank.get_minimum_balance_for_rent_exemption(data.len());
                                let created_program_account = Account {
                                    lamports,
                                    data,
                                    owner: bpf_loader_upgradeable::id(),
                                    executable: true,
                                    rent_epoch: source_account.rent_epoch(),
                                };

                                datapoint_info!(datapoint_name, ("slot", bank.slot, i64));
                                let change_in_capitalization =
                                    source_account.lamports().saturating_sub(lamports);

                                // Replace the destination data account with the source one
                                // If the destination data account does not exist, it will be created
                                // If it does exist, it will be overwritten
                                move_account(
                                    bank,
                                    &source_data_address,
                                    &source_data_account,
                                    &destination_data_address,
                                    bank.get_account_with_fixed_root(&destination_data_address)
                                        .as_ref(),
                                );

                                // Write the source data account's PDA into the destination program account
                                move_account(
                                    bank,
                                    source_address,
                                    &created_program_account,
                                    destination_address,
                                    None::<&AccountSharedData>,
                                );

                                // Any remaining lamports in the source program account are burnt
                                bank.capitalization
                                    .fetch_sub(change_in_capitalization, Relaxed);
                            } else {
                                error!(
                                    "Unable to serialize program account state. \
                                    Source program: {} Source data account: {} \
                                    Destination program: {} Destination data account: {}",
                                    source_address,
                                    source_data_address,
                                    destination_address,
                                    destination_data_address,
                                );
                                datapoint_error!(datapoint_name, ("slot", bank.slot(), i64),);
                            }
                        }
                    } else {
                        warn!(
                            "Unable to find data account for source program. \
                            Source program: {} Source data account: {} \
                            Destination program: {} Destination data account: {}",
                            source_address,
                            source_data_address,
                            destination_address,
                            destination_data_address,
                        );
                        datapoint_warn!(datapoint_name, ("slot", bank.slot(), i64),);
                    }
                }
                _ => {
                    warn!(
                        "Source program account is not an upgradeable program. \
                        Source program: {} Source data account: {} \
                        Destination program: {} Destination data account: {}",
                        source_address,
                        source_data_address,
                        destination_address,
                        destination_data_address,
                    );
                    datapoint_warn!(datapoint_name, ("slot", bank.slot(), i64),);
                    return;
                }
            }
        }
    } else {
        warn!(
            "Unable to find source program {}. Destination: {}",
            source_address, destination_address
        );
        datapoint_warn!(datapoint_name, ("slot", bank.slot(), i64),);
    }
}
