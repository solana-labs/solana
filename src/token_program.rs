//! ERC20-like Token program
use solana_sdk::account::Account;
use solana_sdk::native_loader;
use solana_sdk::pubkey::Pubkey;

const ERC20_NAME: &str = "solana_erc20";
const ERC20_PROGRAM_ID: [u8; 32] = [
    131, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0,
];

pub fn id() -> Pubkey {
    Pubkey::new(&ERC20_PROGRAM_ID)
}

pub fn account() -> Account {
    Account {
        tokens: 1,
        owner: id(),
        userdata: ERC20_NAME.as_bytes().to_vec(),
        executable: true,
        loader: native_loader::id(),
    }
}
