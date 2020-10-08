use crate::{args::DistributeTokensArgs, commands::Error};
use solana_account_decoder::parse_token::pubkey_from_spl_token_v2_0;
use solana_banks_client::{BanksClient, BanksClientExt};
use spl_token_v2_0::{
    solana_program::program_pack::Pack,
    state::{Account as SplTokenAccount, Mint},
};

pub async fn update_token_args(
    client: &mut BanksClient,
    args: &mut DistributeTokensArgs,
) -> Result<(), Error> {
    if let Some(spl_token_args) = &mut args.spl_token_args {
        let sender_account = client
            .get_account(spl_token_args.token_account_address)
            .await?
            .unwrap_or_default();
        let mint_address =
            pubkey_from_spl_token_v2_0(&SplTokenAccount::unpack(&sender_account.data)?.mint);
        let mint_account = client.get_account(mint_address).await?.unwrap_or_default();
        let mint = Mint::unpack(&mint_account.data)?;
        spl_token_args.mint = mint_address;
        spl_token_args.decimals = mint.decimals;
    }
    Ok(())
}

pub fn spl_token_amount(amount: f64, decimals: u8) -> u64 {
    (amount * 10_usize.pow(decimals as u32) as f64) as u64
}
