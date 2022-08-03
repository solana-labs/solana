use solana_config_program::ConfigKeys;
use solana_sdk::pubkey::Pubkey;
use solana_vote_program::vote_state::VoteState;

use bincode;
use serde::Deserialize;
use serde_json;
use solana_runtime::accounts_index::ScanConfig;
use solana_runtime::bank_forks::BankForks;
use solana_sdk::account::ReadableAccount;
use solana_sdk::transaction_context::TransactionAccount;
use std::collections::HashMap;
use std::sync::RwLock;
use std::{collections::HashSet, sync::Arc};

/// ValidatorInfo represents selected fields from the config account data.
#[derive(Debug, Default, Deserialize, Clone, Eq, PartialEq)]
pub struct ValidatorInfo {
    pub name: String,
}

pub type IdentityInfoMap = HashMap<Pubkey, ValidatorInfo>;

pub fn map_vote_identity_to_info(
    bank_forks: &Arc<RwLock<BankForks>>,
    vote_pubkeys: &Arc<HashSet<Pubkey>>,
) -> IdentityInfoMap {
    use solana_sdk::config::program as config_program;

    let bank = bank_forks.read().unwrap().working_bank();

    let all_accounts = bank
        .get_program_accounts(&config_program::id(), &ScanConfig::default())
        .expect("failed to get program accounts");

    let default_vote_state = VoteState::default();
    let vote_accounts = bank.vote_accounts();

    let identities: HashSet<Pubkey> = vote_pubkeys
        .iter()
        .filter_map(|vote_pubkey| {
            let (_, vote_account) = vote_accounts.get(vote_pubkey)?;
            let vote_state = vote_account.vote_state();
            let vote_state = vote_state.as_ref().unwrap_or(&default_vote_state);

            Some(vote_state.node_pubkey)
        })
        .collect();

    get_identity_validator_info(&all_accounts, &identities)
}

/// Return a map from validator identity account to ValidatorInfo.
///
/// To get the validator info (the validator metadata, such as name and Keybase
/// username), we have to extract that from the config account that stores the
/// validator info for a particular validator. But there is no way a priori to
/// know the address of the config account for a given validator; the only way
/// is to enumerate all config accounts and then find the one you are looking
/// for. This function builds a map from identity account to validator info.
fn get_identity_validator_info(
    all_accounts: &[TransactionAccount],
    identities: &HashSet<Pubkey>,
) -> HashMap<Pubkey, ValidatorInfo> {
    let mut mapping = HashMap::new();

    // Due to the structure of validator info (config accounts pointing to identity
    // accounts), it is possible for multiple config accounts to describe the same
    // validator. This is invalid, if it happens, we wouldn't know which config
    // account is the right one, so instead of making an arbitrary decision, we
    // ignore all validator infos for that identity.
    let mut bad_identities = HashSet::new();

    for (_, account) in all_accounts {
        if let Some((validator_identity, validator_info)) =
            deserialize_validator_info(account.data())
        {
            // Skip identities we do not care about.
            if !identities.contains(&validator_identity) {
                continue;
            }

            let old_config_addr = mapping.insert(validator_identity, validator_info);
            if old_config_addr.is_some() {
                bad_identities.insert(validator_identity);
            }
        }
    }

    for bad_identity in &bad_identities {
        mapping.remove(bad_identity);
    }

    mapping
}

/// Deserialize a config account that contains validator info.
///
/// Returns the validator identity account address, and the validator info for
/// that validator.
pub fn deserialize_validator_info(account_data: &[u8]) -> Option<(Pubkey, ValidatorInfo)> {
    use solana_account_decoder::validator_info;

    let key_list: ConfigKeys = bincode::deserialize(account_data).ok()?;

    if !key_list.keys.contains(&(validator_info::id(), false)) {
        return None;
    }

    // The validator identity pubkey lives at index 1.
    let (validator_identity, identity_signed_config) = key_list.keys[1];
    if !identity_signed_config {
        return None;
    }

    // A config account stores a list of (pubkey, bool) pairs, followed by json
    // data. To figure out where the json data starts, we need to know the size
    // fo the key list. The json data is not stored directly, it is serialized
    // with bincode as a string.
    let key_list_len = bincode::serialized_size(&key_list)
        .expect("We deserialized it, therefore it must be serializable.")
        as usize;
    let json_data: String = bincode::deserialize(&account_data[key_list_len..]).ok()?;
    let validator_info: ValidatorInfo = serde_json::from_str(&json_data).ok()?;

    Some((validator_identity, validator_info))
}
