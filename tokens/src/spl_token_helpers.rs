use crate::{
    args::DistributeTokensArgs,
    commands::{Allocation, Error, FundingSource},
};
use solana_account_decoder::parse_token::{pubkey_from_spl_token_v2_0, token_amount_to_ui_amount};
use solana_banks_client::{BanksClient, BanksClientExt};
use solana_sdk::native_token::lamports_to_sol;
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

pub async fn check_spl_token_balances(
    num_signatures: usize,
    allocations: &[Allocation],
    client: &mut BanksClient,
    args: &DistributeTokensArgs,
    created_accounts: u64,
) -> Result<(), Error> {
    let spl_token_args = args
        .spl_token_args
        .as_ref()
        .expect("spl_token_args must be some");
    let undistributed_tokens: f64 = allocations.iter().map(|x| x.amount).sum();
    let allocation_amount = spl_token_amount(undistributed_tokens, spl_token_args.decimals);

    let (fee_calculator, _blockhash, _last_valid_slot) = client.get_fees().await?;
    let fees = fee_calculator
        .lamports_per_signature
        .checked_mul(num_signatures as u64)
        .unwrap();

    let rent = client.get_rent().await?;
    let token_account_rent_exempt_balance = rent.minimum_balance(SplTokenAccount::LEN);
    let account_creation_amount = created_accounts * token_account_rent_exempt_balance;
    let fee_payer_balance = client.get_balance(args.fee_payer.pubkey()).await?;
    if fee_payer_balance < fees + account_creation_amount {
        return Err(Error::InsufficientFunds(
            vec![FundingSource::FeePayer].into(),
            lamports_to_sol(fees + account_creation_amount),
        ));
    }
    let source_token_account = client
        .get_account(spl_token_args.token_account_address)
        .await?
        .unwrap_or_default();
    let source_token = SplTokenAccount::unpack(&source_token_account.data)?;
    if source_token.amount < allocation_amount {
        return Err(Error::InsufficientFunds(
            vec![FundingSource::SplTokenAccount].into(),
            token_amount_to_ui_amount(allocation_amount, spl_token_args.decimals).ui_amount,
        ));
    }
    Ok(())
}
