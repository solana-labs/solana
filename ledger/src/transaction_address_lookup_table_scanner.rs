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

pub struct ScannedLookupTableExtensions {
    pub possibly_incomplete: bool,
    pub accounts: Vec<Pubkey>, // empty if no extensions found
}

pub fn scan_transaction(
    transaction: &SanitizedVersionedTransaction,
) -> ScannedLookupTableExtensions {
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

    ScannedLookupTableExtensions {
        possibly_incomplete: !native_only,
        accounts,
    }
}
