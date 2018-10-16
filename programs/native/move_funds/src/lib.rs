extern crate bincode;
extern crate solana_program_interface;

use bincode::deserialize;
use solana_program_interface::account::KeyedAccount;

#[no_mangle]
pub extern "C" fn process(keyed_accounts: &mut Vec<KeyedAccount>, data: &[u8]) -> bool {
    let tokens: i64 = deserialize(data).unwrap();
    if keyed_accounts[0].account.tokens >= tokens {
        keyed_accounts[0].account.tokens -= tokens;
        keyed_accounts[1].account.tokens += tokens;
        true
    } else {
        println!(
            "Insufficient funds, asked {}, only had {}",
            tokens, keyed_accounts[0].account.tokens
        );
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bincode::serialize;
    use solana_program_interface::account::Account;
    use solana_program_interface::pubkey::Pubkey;

    #[test]
    fn test_move_funds() {
        let tokens: i64 = 100;
        let data: Vec<u8> = serialize(&tokens).unwrap();
        let keys = vec![Pubkey::default(); 2];
        let mut accounts = vec![Account::default(), Account::default()];
        accounts[0].tokens = 100;
        accounts[1].tokens = 1;

        {
            let mut keyed_accounts: Vec<KeyedAccount> = Vec::new();
            for (key, account) in keys.iter().zip(&mut accounts).collect::<Vec<_>>() {
                infos.push(KeyedAccount { key, account });
            }

            process(&mut keyed_accounts, &data);
        }
        assert_eq!(0, accounts[0].tokens);
        assert_eq!(101, accounts[1].tokens);
    }
}
