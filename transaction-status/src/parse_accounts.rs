use serde_json::{json, Value};
use solana_sdk::message::Message;

type AccountAttributes = Vec<AccountAttribute>;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum AccountAttribute {
    Signer,
    Writable,
}

pub fn parse_accounts(message: &Message) -> Value {
    let mut accounts: Vec<Value> = vec![];
    for (i, account_key) in message.account_keys.iter().enumerate() {
        let mut attributes: AccountAttributes = vec![];
        if message.is_writable(i) {
            attributes.push(AccountAttribute::Writable);
        }
        if message.is_signer(i) {
            attributes.push(AccountAttribute::Signer);
        }
        accounts.push(json!({ account_key.to_string(): attributes }));
    }
    json!(accounts)
}

#[cfg(test)]
mod test {
    use super::*;
    use solana_sdk::{message::MessageHeader, pubkey::Pubkey};

    #[test]
    fn test_parse_accounts() {
        let pubkey0 = Pubkey::new_rand();
        let pubkey1 = Pubkey::new_rand();
        let pubkey2 = Pubkey::new_rand();
        let pubkey3 = Pubkey::new_rand();
        let mut message = Message::default();
        message.header = MessageHeader {
            num_required_signatures: 2,
            num_readonly_signed_accounts: 1,
            num_readonly_unsigned_accounts: 1,
        };
        message.account_keys = vec![pubkey0, pubkey1, pubkey2, pubkey3];

        let expected_json = json!([
            {pubkey0.to_string(): ["writable", "signer"]},
            {pubkey1.to_string(): ["signer"]},
            {pubkey2.to_string(): ["writable"]},
            {pubkey3.to_string(): []},
        ]);

        assert_eq!(parse_accounts(&message), expected_json);
    }
}
