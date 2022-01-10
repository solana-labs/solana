use solana_sdk::message::Message;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
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

#[cfg(test)]
mod test {
    use {super::*, solana_sdk::message::MessageHeader};

    #[test]
    fn test_parse_accounts() {
        let pubkey0 = solana_sdk::pubkey::new_rand();
        let pubkey1 = solana_sdk::pubkey::new_rand();
        let pubkey2 = solana_sdk::pubkey::new_rand();
        let pubkey3 = solana_sdk::pubkey::new_rand();
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
}
