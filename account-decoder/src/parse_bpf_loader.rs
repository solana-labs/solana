use {
    crate::{
        parse_account_data::{ParsableAccount, ParseAccountError},
        UiAccountData, UiAccountEncoding,
    },
    base64::{prelude::BASE64_STANDARD, Engine},
    bincode::{deserialize, serialized_size},
    solana_sdk::{bpf_loader_upgradeable::UpgradeableLoaderState, pubkey::Pubkey},
};

pub fn parse_bpf_upgradeable_loader(
    data: &[u8],
) -> Result<BpfUpgradeableLoaderAccountType, ParseAccountError> {
    let account_state: UpgradeableLoaderState = deserialize(data).map_err(|_| {
        ParseAccountError::AccountNotParsable(ParsableAccount::BpfUpgradeableLoader)
    })?;
    let parsed_account = match account_state {
        UpgradeableLoaderState::Uninitialized => BpfUpgradeableLoaderAccountType::Uninitialized,
        UpgradeableLoaderState::Buffer { authority_address } => {
            let offset = if authority_address.is_some() {
                UpgradeableLoaderState::size_of_buffer_metadata()
            } else {
                // This case included for code completeness; in practice, a Buffer account will
                // always have authority_address.is_some()
                UpgradeableLoaderState::size_of_buffer_metadata()
                    - serialized_size(&Pubkey::default()).unwrap() as usize
            };
            BpfUpgradeableLoaderAccountType::Buffer(UiBuffer {
                authority: authority_address.map(|pubkey| pubkey.to_string()),
                data: UiAccountData::Binary(
                    BASE64_STANDARD.encode(&data[offset..]),
                    UiAccountEncoding::Base64,
                ),
            })
        }
        UpgradeableLoaderState::Program {
            programdata_address,
        } => BpfUpgradeableLoaderAccountType::Program(UiProgram {
            program_data: programdata_address.to_string(),
        }),
        UpgradeableLoaderState::ProgramData {
            slot,
            upgrade_authority_address,
        } => {
            let offset = if upgrade_authority_address.is_some() {
                UpgradeableLoaderState::size_of_programdata_metadata()
            } else {
                UpgradeableLoaderState::size_of_programdata_metadata()
                    - serialized_size(&Pubkey::default()).unwrap() as usize
            };
            BpfUpgradeableLoaderAccountType::ProgramData(UiProgramData {
                slot,
                authority: upgrade_authority_address.map(|pubkey| pubkey.to_string()),
                data: UiAccountData::Binary(
                    BASE64_STANDARD.encode(&data[offset..]),
                    UiAccountEncoding::Base64,
                ),
            })
        }
    };
    Ok(parsed_account)
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", tag = "type", content = "info")]
pub enum BpfUpgradeableLoaderAccountType {
    Uninitialized,
    Buffer(UiBuffer),
    Program(UiProgram),
    ProgramData(UiProgramData),
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiBuffer {
    pub authority: Option<String>,
    pub data: UiAccountData,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiProgram {
    pub program_data: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiProgramData {
    pub slot: u64,
    pub authority: Option<String>,
    pub data: UiAccountData,
}

#[cfg(test)]
mod test {
    use {super::*, bincode::serialize, solana_sdk::pubkey::Pubkey};

    #[test]
    fn test_parse_bpf_upgradeable_loader_accounts() {
        let bpf_loader_state = UpgradeableLoaderState::Uninitialized;
        let account_data = serialize(&bpf_loader_state).unwrap();
        assert_eq!(
            parse_bpf_upgradeable_loader(&account_data).unwrap(),
            BpfUpgradeableLoaderAccountType::Uninitialized
        );

        let program = vec![7u8; 64]; // Arbitrary program data

        let authority = Pubkey::new_unique();
        let bpf_loader_state = UpgradeableLoaderState::Buffer {
            authority_address: Some(authority),
        };
        let mut account_data = serialize(&bpf_loader_state).unwrap();
        account_data.extend_from_slice(&program);
        assert_eq!(
            parse_bpf_upgradeable_loader(&account_data).unwrap(),
            BpfUpgradeableLoaderAccountType::Buffer(UiBuffer {
                authority: Some(authority.to_string()),
                data: UiAccountData::Binary(
                    BASE64_STANDARD.encode(&program),
                    UiAccountEncoding::Base64
                ),
            })
        );

        // This case included for code completeness; in practice, a Buffer account will always have
        // authority_address.is_some()
        let bpf_loader_state = UpgradeableLoaderState::Buffer {
            authority_address: None,
        };
        let mut account_data = serialize(&bpf_loader_state).unwrap();
        account_data.extend_from_slice(&program);
        assert_eq!(
            parse_bpf_upgradeable_loader(&account_data).unwrap(),
            BpfUpgradeableLoaderAccountType::Buffer(UiBuffer {
                authority: None,
                data: UiAccountData::Binary(
                    BASE64_STANDARD.encode(&program),
                    UiAccountEncoding::Base64
                ),
            })
        );

        let programdata_address = Pubkey::new_unique();
        let bpf_loader_state = UpgradeableLoaderState::Program {
            programdata_address,
        };
        let account_data = serialize(&bpf_loader_state).unwrap();
        assert_eq!(
            parse_bpf_upgradeable_loader(&account_data).unwrap(),
            BpfUpgradeableLoaderAccountType::Program(UiProgram {
                program_data: programdata_address.to_string(),
            })
        );

        let authority = Pubkey::new_unique();
        let slot = 42;
        let bpf_loader_state = UpgradeableLoaderState::ProgramData {
            slot,
            upgrade_authority_address: Some(authority),
        };
        let mut account_data = serialize(&bpf_loader_state).unwrap();
        account_data.extend_from_slice(&program);
        assert_eq!(
            parse_bpf_upgradeable_loader(&account_data).unwrap(),
            BpfUpgradeableLoaderAccountType::ProgramData(UiProgramData {
                slot,
                authority: Some(authority.to_string()),
                data: UiAccountData::Binary(
                    BASE64_STANDARD.encode(&program),
                    UiAccountEncoding::Base64
                ),
            })
        );

        let bpf_loader_state = UpgradeableLoaderState::ProgramData {
            slot,
            upgrade_authority_address: None,
        };
        let mut account_data = serialize(&bpf_loader_state).unwrap();
        account_data.extend_from_slice(&program);
        assert_eq!(
            parse_bpf_upgradeable_loader(&account_data).unwrap(),
            BpfUpgradeableLoaderAccountType::ProgramData(UiProgramData {
                slot,
                authority: None,
                data: UiAccountData::Binary(
                    BASE64_STANDARD.encode(&program),
                    UiAccountEncoding::Base64
                ),
            })
        );
    }
}
