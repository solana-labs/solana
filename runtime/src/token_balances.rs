use crate::bank::Bank;
use solana_account_decoder::parse_token::{
    spl_token_id_v2_0, spl_token_v2_0_native_mint, token_amount_to_ui_amount, UiTokenAmount,
};
use solana_sdk::pubkey::Pubkey;
use spl_token_v2_0::{
    solana_sdk::program_pack::Pack,
    state::{Account as TokenAccount, Mint},
};
use std::str::FromStr;

pub fn is_token_program(program_id: &Pubkey) -> bool {
    return program_id == &spl_token_id_v2_0();
}

fn get_mint_owner_and_decimals(bank: &Bank, mint: &Pubkey) -> Option<(Pubkey, u8)> {
    if mint == &spl_token_v2_0_native_mint() {
        Some((spl_token_id_v2_0(), spl_token_v2_0::native_mint::DECIMALS))
    } else {
        let mint_account = bank.get_account(mint)?;

        let decimals = Mint::unpack(&mint_account.data)
            .map(|mint| mint.decimals)
            .ok()?;

        Some((mint_account.owner, decimals))
    }
}

pub fn collect_token_balance_from_account(
    bank: &Bank,
    account_id: &Pubkey,
) -> Option<(String, UiTokenAmount)> {
    let account = bank.get_account(account_id)?;

    let token_account = TokenAccount::unpack(&account.data).ok()?;
    let mint_string = &token_account.mint.to_string();
    let mint = &Pubkey::from_str(&mint_string).unwrap_or_default();

    let (_, decimals) = get_mint_owner_and_decimals(bank, &mint)?;

    Some((
        mint_string.to_string(),
        token_amount_to_ui_amount(token_account.amount, decimals),
    ))
}
