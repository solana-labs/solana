use {
    bincode::deserialize,
    lazy_static::lazy_static,
    solana_sdk::{
        address_lookup_table::{self, instruction::ProgramInstruction},
        pubkey::Pubkey,
        sdk_ids::SDK_IDS,
        transaction::SanitizedVersionedTransaction,
    },
    std::collections::HashSet,
};

lazy_static! {
    static ref SDK_IDS_SET: HashSet<Pubkey> = SDK_IDS.iter().cloned().collect();
}

pub enum ScanResult {
    /// The address lookup table program was not used
    NotFound,
    /// The address lookup table program was present, with non-native programs.
    /// If instructions extending an account lookup table were found, the list
    /// of Pubkeys will be returned. However, this may be incomplete due to the
    /// possibility of non-native ixs CPIing to extend.
    NonNativeUsed(Vec<Pubkey>),
    /// The address lookup table program was present, with only native programs.
    /// If instructions extending an account lookup table were found, the list
    /// of Pubkeys will be returned.
    NativeUsed(Vec<Pubkey>),
}

pub fn scan_transaction(transaction: &SanitizedVersionedTransaction) -> ScanResult {
    // If the ALT program is not present in the account keys, it was not used
    if !transaction
        .get_message()
        .message
        .static_account_keys()
        .iter()
        .any(address_lookup_table::program::check_id)
    {
        return ScanResult::NotFound;
    }

    // Accumulate accounts from account lookup table extension instructions
    let mut accounts = Vec::new();
    let mut native_only = true;
    for (program_id, instruction) in transaction.get_message().program_instructions_iter() {
        if address_lookup_table::program::check_id(program_id) {
            if let Ok(ProgramInstruction::ExtendLookupTable { new_addresses }) =
                deserialize::<ProgramInstruction>(&instruction.data)
            {
                accounts.extend(new_addresses);
            }
        } else {
            native_only &= SDK_IDS_SET.contains(program_id);
        }
    }

    if native_only {
        ScanResult::NativeUsed(accounts)
    } else {
        ScanResult::NonNativeUsed(accounts)
    }
}
