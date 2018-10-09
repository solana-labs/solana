extern crate bincode;
extern crate rlua;
extern crate solana_program_interface;

use bincode::deserialize;
use rlua::{Lua, Result, Table};
use solana_program_interface::account::KeyedAccount;

/// Make KeyAccount values available to Lua.
fn set_accounts(lua: &Lua, name: &str, keyed_accounts: &[KeyedAccount]) -> Result<()> {
    let accounts = lua.create_table()?;
    for (i, keyed_account) in keyed_accounts.iter().enumerate() {
        let account = lua.create_table()?;
        account.set("tokens", keyed_account.account.tokens)?;
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
    }
    Ok(())
}

fn run_lua(keyed_accounts: &mut Vec<KeyedAccount>, code: &str) -> Result<()> {
    let lua = Lua::new();
    set_accounts(&lua, "accounts", keyed_accounts)?;
    lua.exec::<_, ()>(code, None)?;
    update_accounts(&lua, "accounts", keyed_accounts)
}

#[no_mangle]
pub extern "C" fn process(keyed_accounts: &mut Vec<KeyedAccount>, data: &[u8]) {
    let code: String = deserialize(&data).unwrap();
    run_lua(keyed_accounts, &code).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;
    use bincode::serialize;
    use solana_program_interface::account::{create_keyed_accounts, Account};
    use solana_program_interface::pubkey::Pubkey;

    #[test]
    fn test_update_accounts() -> Result<()> {
        let mut accounts = [(Pubkey::default(), Account::default())];
        let mut keyed_accounts = create_keyed_accounts(&mut accounts);
        let lua = Lua::new();
        set_accounts(&lua, "xs", &keyed_accounts)?;
        keyed_accounts[0].account.tokens = 42;
        update_accounts(&lua, "xs", &mut keyed_accounts)?;

        // Ensure update_accounts() overwrites the local value 42.
        assert_eq!(keyed_accounts[0].account.tokens, 0);

        Ok(())
    }

    #[test]
    fn test_credit_with_lua() -> Result<()> {
        let code = r#"accounts[1].tokens = accounts[1].tokens + 1"#;
        let mut accounts = [(Pubkey::default(), Account::default())];
        run_lua(&mut create_keyed_accounts(&mut accounts), code)?;
        assert_eq!(accounts[0].1.tokens, 1);
        Ok(())
    }

    #[test]
    fn test_error_with_lua() {
        let code = r#"accounts[1].tokens += 1"#;
        let mut accounts = [(Pubkey::default(), Account::default())];
        assert!(run_lua(&mut create_keyed_accounts(&mut accounts), code).is_err());
    }

    #[test]
    fn test_move_funds_with_lua_via_process() {
        let data: Vec<u8> = serialize(
            r#"
            local tokens = 100
            accounts[1].tokens = accounts[1].tokens - tokens
            accounts[2].tokens = accounts[2].tokens + tokens
        "#,
        ).unwrap();

        let alice_pubkey = Pubkey::default();
        let bob_pubkey = Pubkey::default();
        let program_id = Pubkey::default();

        let mut accounts = [
            (alice_pubkey, Account::new(100, 0, program_id)),
            (bob_pubkey, Account::new(1, 0, program_id)),
        ];
        process(&mut create_keyed_accounts(&mut accounts), &data);

        assert_eq!(accounts[0].1.tokens, 0);
        assert_eq!(accounts[1].1.tokens, 101);
    }
}
