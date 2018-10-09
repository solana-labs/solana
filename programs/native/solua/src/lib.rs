extern crate rlua;
extern crate solana_program_interface;

use rlua::{Lua, Result, Table};
use solana_program_interface::account::KeyedAccount;
use std::str;

/// Make KeyAccount values available to Lua.
fn set_accounts(lua: &Lua, name: &str, keyed_accounts: &[KeyedAccount]) -> Result<()> {
    let accounts = lua.create_table()?;
    for (i, keyed_account) in keyed_accounts.iter().enumerate() {
        let account = lua.create_table()?;
        account.set("tokens", keyed_account.account.tokens)?;
        let data_str = lua.create_string(&keyed_account.account.userdata)?;
        account.set("userdata", data_str)?;
        accounts.set(i + 1, account)?;
    }
    let globals = lua.globals();
    globals.set(name, accounts)
}

/// Commit the new KeyedAccount values.
fn update_accounts(lua: &Lua, name: &str, keyed_accounts: &mut Vec<KeyedAccount>) -> Result<()> {
    let globals = lua.globals();
    let accounts: Table = globals.get(name)?;
    for (i, keyed_account) in keyed_accounts.into_iter().enumerate() {
        let account: Table = accounts.get(i + 1)?;
        keyed_account.account.tokens = account.get("tokens")?;
        let data_str: rlua::String = account.get("userdata")?;
        keyed_account.account.userdata = data_str.as_bytes().to_vec();
    }
    Ok(())
}

fn run_lua(keyed_accounts: &mut Vec<KeyedAccount>, code: &str, data: &[u8]) -> Result<()> {
    let lua = Lua::new();
    let globals = lua.globals();
    let data_str = lua.create_string(data)?;
    globals.set("data", data_str)?;

    set_accounts(&lua, "accounts", keyed_accounts)?;
    lua.exec::<_, ()>(code, None)?;
    update_accounts(&lua, "accounts", keyed_accounts)
}

#[no_mangle]
pub extern "C" fn process(keyed_accounts: &mut Vec<KeyedAccount>, data: &[u8]) {
    let code_data = keyed_accounts[0].account.userdata.clone();
    let code = str::from_utf8(&code_data).unwrap();
    run_lua(keyed_accounts, &code, data).unwrap();
}

#[cfg(test)]
mod tests {
    extern crate bincode;

    use self::bincode::serialize;
    use super::*;
    use solana_program_interface::account::{create_keyed_accounts, Account};
    use solana_program_interface::pubkey::Pubkey;

    #[test]
    fn test_update_accounts() -> Result<()> {
        let mut accounts = [(Pubkey::default(), Account::default())];
        let mut keyed_accounts = create_keyed_accounts(&mut accounts);
        let lua = Lua::new();
        set_accounts(&lua, "xs", &keyed_accounts)?;
        keyed_accounts[0].account.tokens = 42;
        keyed_accounts[0].account.userdata = vec![];
        update_accounts(&lua, "xs", &mut keyed_accounts)?;

        // Ensure update_accounts() overwrites the local value 42.
        assert_eq!(keyed_accounts[0].account.tokens, 0);

        Ok(())
    }

    #[test]
    fn test_credit_with_lua() -> Result<()> {
        let code = r#"accounts[1].tokens = accounts[1].tokens + 1"#;
        let mut accounts = [(Pubkey::default(), Account::default())];
        run_lua(&mut create_keyed_accounts(&mut accounts), code, &[])?;
        assert_eq!(accounts[0].1.tokens, 1);
        Ok(())
    }

    #[test]
    fn test_error_with_lua() {
        let code = r#"accounts[1].tokens += 1"#;
        let mut accounts = [(Pubkey::default(), Account::default())];
        assert!(run_lua(&mut create_keyed_accounts(&mut accounts), code, &[]).is_err());
    }

    #[test]
    fn test_move_funds_with_lua_via_process() {
        let userdata = r#"
            local tokens, _ = string.unpack("I", data)
            accounts[1].tokens = accounts[1].tokens - tokens
            accounts[2].tokens = accounts[2].tokens + tokens
        "#.as_bytes()
        .to_vec();

        let alice_pubkey = Pubkey::default();
        let bob_pubkey = Pubkey::default();
        let program_id = Pubkey::default();

        let mut accounts = [
            (
                alice_pubkey,
                Account {
                    tokens: 100,
                    userdata,
                    program_id,
                },
            ),
            (bob_pubkey, Account::new(1, 0, program_id)),
        ];
        let data = serialize(&10u64).unwrap();
        process(&mut create_keyed_accounts(&mut accounts), &data);
        assert_eq!(accounts[0].1.tokens, 90);
        assert_eq!(accounts[1].1.tokens, 11);

        process(&mut create_keyed_accounts(&mut accounts), &data);
        assert_eq!(accounts[0].1.tokens, 80);
        assert_eq!(accounts[1].1.tokens, 21);
    }

    #[test]
    fn test_move_funds_with_lua_via_process_and_terminate() {
        let userdata = r#"
            local tokens, _ = string.unpack("I", data)
            accounts[1].tokens = accounts[1].tokens - tokens
            accounts[2].tokens = accounts[2].tokens + tokens
            accounts[1].userdata = ""
        "#.as_bytes()
        .to_vec();

        let alice_pubkey = Pubkey::default();
        let bob_pubkey = Pubkey::default();
        let program_id = Pubkey::default();

        let mut accounts = [
            (
                alice_pubkey,
                Account {
                    tokens: 100,
                    userdata,
                    program_id,
                },
            ),
            (bob_pubkey, Account::new(1, 0, program_id)),
        ];
        let data = serialize(&10).unwrap();
        process(&mut create_keyed_accounts(&mut accounts), &data);
        assert_eq!(accounts[0].1.tokens, 90);
        assert_eq!(accounts[1].1.tokens, 11);

        // Verify the program modified itself to a no-op.
        process(&mut create_keyed_accounts(&mut accounts), &data);
        assert_eq!(accounts[0].1.tokens, 90);
        assert_eq!(accounts[1].1.tokens, 11);
    }
}
