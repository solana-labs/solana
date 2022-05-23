use solana_sdk::message::{v0::LoadedMessage, Message};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ParsedAccount {
    pub pubkey: String,
    pub writable: bool,
    pub signer: bool,
}

pub fn parse_accounts(message: &Message) -> Vec<ParsedAccount> {
    let mut accounts: Vec<ParsedAccount> = vec![];
    for (i, account_key) in message.account_keys.iter().enumerate() {
        accounts.push(ParsedAccount {
            pubkey: account_key.to_string(),
            writable: message.is_writable(i),
            signer: message.is_signer(i),
        });
    }
    accounts
}

pub fn parse_static_accounts(message: &LoadedMessage) -> Vec<ParsedAccount> {
    let mut accounts: Vec<ParsedAccount> = vec![];
    for (i, account_key) in message.static_account_keys().iter().enumerate() {
        accounts.push(ParsedAccount {
            pubkey: account_key.to_string(),
            writable: message.is_writable(i),
            signer: message.is_signer(i),
        });
    }
    accounts
}

#[cfg(test)]
mod test {
    use {
        super::*,
        solana_sdk::{
            message::{v0, v0::LoadedAddresses, MessageHeader},
            pubkey::Pubkey,
        },
    };

    #[test]
    fn test_parse_accounts() {
        let pubkey0 = Pubkey::new_unique();
        let pubkey1 = Pubkey::new_unique();
        let pubkey2 = Pubkey::new_unique();
        let pubkey3 = Pubkey::new_unique();
        let message = Message {
            header: MessageHeader {
                num_required_signatures: 2,
                num_readonly_signed_accounts: 1,
                num_readonly_unsigned_accounts: 1,
            },
            account_keys: vec![pubkey0, pubkey1, pubkey2, pubkey3],
            ..Message::default()
        };

        assert_eq!(
            parse_accounts(&message),
            vec![
                ParsedAccount {
                    pubkey: pubkey0.to_string(),
                    writable: true,
                    signer: true,
                },
                ParsedAccount {
                    pubkey: pubkey1.to_string(),
                    writable: false,
                    signer: true,
                },
                ParsedAccount {
                    pubkey: pubkey2.to_string(),
                    writable: true,
                    signer: false,
                },
                ParsedAccount {
                    pubkey: pubkey3.to_string(),
                    writable: false,
                    signer: false,
                },
            ]
        );
    }

    #[test]
    fn test_parse_static_accounts() {
        let pubkey0 = Pubkey::new_unique();
        let pubkey1 = Pubkey::new_unique();
        let pubkey2 = Pubkey::new_unique();
        let pubkey3 = Pubkey::new_unique();
        let message = LoadedMessage::new(
            v0::Message {
                header: MessageHeader {
                    num_required_signatures: 2,
                    num_readonly_signed_accounts: 1,
                    num_readonly_unsigned_accounts: 1,
                },
                account_keys: vec![pubkey0, pubkey1, pubkey2, pubkey3],
                ..v0::Message::default()
            },
            LoadedAddresses {
                writable: vec![Pubkey::new_unique()],
                readonly: vec![Pubkey::new_unique()],
            },
        );

        assert_eq!(
            parse_static_accounts(&message),
            vec![
                ParsedAccount {
                    pubkey: pubkey0.to_string(),
                    writable: true,
                    signer: true,
                },
                ParsedAccount {
                    pubkey: pubkey1.to_string(),
                    writable: false,
                    signer: true,
                },
                ParsedAccount {
                    pubkey: pubkey2.to_string(),
                    writable: true,
                    signer: false,
                },
                ParsedAccount {
                    pubkey: pubkey3.to_string(),
                    writable: false,
                    signer: false,
                },
            ]
        );
    }
}
