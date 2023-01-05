use {
    crate::parse_account_data::{ParsableAccount, ParseAccountError},
    solana_address_lookup_table_program::state::AddressLookupTable,
    solana_sdk::instruction::InstructionError,
};

pub fn parse_address_lookup_table(
    data: &[u8],
) -> Result<LookupTableAccountType, ParseAccountError> {
    AddressLookupTable::deserialize(data)
        .map(|address_lookup_table| {
            LookupTableAccountType::LookupTable(address_lookup_table.into())
        })
        .or_else(|err| match err {
            InstructionError::UninitializedAccount => Ok(LookupTableAccountType::Uninitialized),
            _ => Err(ParseAccountError::AccountNotParsable(
                ParsableAccount::AddressLookupTable,
            )),
        })
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", tag = "type", content = "info")]
pub enum LookupTableAccountType {
    Uninitialized,
    LookupTable(UiLookupTable),
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiLookupTable {
    pub deactivation_slot: String,
    pub last_extended_slot: String,
    pub last_extended_slot_start_index: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authority: Option<String>,
    pub addresses: Vec<String>,
}

impl<'a> From<AddressLookupTable<'a>> for UiLookupTable {
    fn from(address_lookup_table: AddressLookupTable) -> Self {
        Self {
            deactivation_slot: address_lookup_table.meta.deactivation_slot.to_string(),
            last_extended_slot: address_lookup_table.meta.last_extended_slot.to_string(),
            last_extended_slot_start_index: address_lookup_table
                .meta
                .last_extended_slot_start_index,
            authority: address_lookup_table
                .meta
                .authority
                .map(|authority| authority.to_string()),
            addresses: address_lookup_table
                .addresses
                .iter()
                .map(|address| address.to_string())
                .collect(),
        }
    }
}

#[cfg(test)]
mod test {
    use {
        super::*,
        solana_address_lookup_table_program::state::{LookupTableMeta, LOOKUP_TABLE_META_SIZE},
        solana_sdk::pubkey::Pubkey,
        std::borrow::Cow,
    };

    #[test]
    fn test_parse_address_lookup_table() {
        let authority = Pubkey::new_unique();
        let deactivation_slot = 1;
        let last_extended_slot = 2;
        let last_extended_slot_start_index = 3;
        let lookup_table_meta = LookupTableMeta {
            deactivation_slot,
            last_extended_slot,
            last_extended_slot_start_index,
            authority: Some(authority),
            ..LookupTableMeta::default()
        };
        let num_addresses = 42;
        let mut addresses = Vec::with_capacity(num_addresses);
        addresses.resize_with(num_addresses, Pubkey::new_unique);
        let lookup_table = AddressLookupTable {
            meta: lookup_table_meta,
            addresses: Cow::Owned(addresses),
        };
        let lookup_table_data = AddressLookupTable::serialize_for_tests(lookup_table).unwrap();

        let parsing_result = parse_address_lookup_table(&lookup_table_data).unwrap();
        if let LookupTableAccountType::LookupTable(ui_lookup_table) = parsing_result {
            assert_eq!(
                ui_lookup_table.deactivation_slot,
                deactivation_slot.to_string()
            );
            assert_eq!(
                ui_lookup_table.last_extended_slot,
                last_extended_slot.to_string()
            );
            assert_eq!(
                ui_lookup_table.last_extended_slot_start_index,
                last_extended_slot_start_index
            );
            assert_eq!(ui_lookup_table.authority, Some(authority.to_string()));
            assert_eq!(ui_lookup_table.addresses.len(), num_addresses);
        }

        assert_eq!(
            parse_address_lookup_table(&[0u8; LOOKUP_TABLE_META_SIZE]).unwrap(),
            LookupTableAccountType::Uninitialized
        );
        assert!(parse_address_lookup_table(&[]).is_err());
    }
}
