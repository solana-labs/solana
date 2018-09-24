extern crate bincode;
extern crate solana;

use bincode::deserialize;
use solana::dynamic_program::KeyedAccount;

#[no_mangle]
pub extern "C" fn process(infos: &mut Vec<KeyedAccount>, data: &[u8]) {
    let tokens: i64 = deserialize(data).unwrap();
    if infos[0].account.tokens >= tokens {
        infos[0].account.tokens -= tokens;
        infos[1].account.tokens += tokens;
    } else {
        println!(
            "Insufficient funds, asked {}, only had {}",
            tokens, infos[0].account.tokens
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bincode::serialize;
    use solana::bank::Account;
    use solana::signature::Pubkey;

    #[test]
    fn test_move_funds() {
        let tokens: i64 = 100;
        let data: Vec<u8> = serialize(&tokens).unwrap();
        let keys = vec![Pubkey::default(); 2];
        let mut accounts = vec![Account::default(), Account::default()];
        accounts[0].tokens = 100;
        accounts[1].tokens = 1;

        {
            let mut infos: Vec<KeyedAccount> = Vec::new();
            for (key, account) in keys.iter().zip(&mut accounts).collect::<Vec<_>>() {
                infos.push(KeyedAccount { key, account });
            }

            process(&mut infos, &data);
        }
        assert_eq!(0, accounts[0].tokens);
        assert_eq!(101, accounts[1].tokens);
    }
}
