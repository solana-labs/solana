use crate::{
    args::{DistributeTokensArgs, SplTokenArgs},
    commands::{Allocation, Error, FundingSource},
};
use console::style;
use solana_account_decoder::parse_token::{
    pubkey_from_spl_token_v2_0, spl_token_v2_0_pubkey, token_amount_to_ui_amount,
};
use solana_client::rpc_client::RpcClient;
use solana_sdk::{instruction::Instruction, native_token::lamports_to_sol};
use solana_transaction_status::parse_token::spl_token_v2_0_instruction;
use spl_associated_token_account_v1_0::{
    create_associated_token_account, get_associated_token_address,
};
use spl_token_v2_0::{
    solana_program::program_pack::Pack,
    state::{Account as SplTokenAccount, Mint},
};

pub fn update_token_args(client: &RpcClient, args: &mut Option<SplTokenArgs>) -> Result<(), Error> {
    if let Some(spl_token_args) = args {
        let sender_account = client
            .get_account(&spl_token_args.token_account_address)
            .unwrap_or_default();
        let mint_address =
            pubkey_from_spl_token_v2_0(&SplTokenAccount::unpack(&sender_account.data)?.mint);
        spl_token_args.mint = mint_address;
        update_decimals(client, args)?;
    }
    Ok(())
}

pub fn update_decimals(client: &RpcClient, args: &mut Option<SplTokenArgs>) -> Result<(), Error> {
    if let Some(spl_token_args) = args {
        let mint_account = client.get_account(&spl_token_args.mint).unwrap_or_default();
        let mint = Mint::unpack(&mint_account.data)?;
        spl_token_args.decimals = mint.decimals;
    }
    Ok(())
}

pub fn spl_token_amount(amount: f64, decimals: u8) -> u64 {
    (amount * 10_usize.pow(decimals as u32) as f64) as u64
}

pub fn build_spl_token_instructions(
    allocation: &Allocation,
    args: &DistributeTokensArgs,
    do_create_associated_token_account: bool,
) -> Vec<Instruction> {
    let spl_token_args = args
        .spl_token_args
        .as_ref()
        .expect("spl_token_args must be some");
    let wallet_address = allocation.recipient.parse().unwrap();
    let associated_token_address = get_associated_token_address(
        &wallet_address,
        &spl_token_v2_0_pubkey(&spl_token_args.mint),
    );
    let mut instructions = vec![];
    if do_create_associated_token_account {
        let create_associated_token_account_instruction = create_associated_token_account(
            &spl_token_v2_0_pubkey(&args.fee_payer.pubkey()),
            &wallet_address,
            &spl_token_v2_0_pubkey(&spl_token_args.mint),
        );
        instructions.push(spl_token_v2_0_instruction(
            create_associated_token_account_instruction,
        ));
    }
    let spl_instruction = spl_token_v2_0::instruction::transfer_checked(
        &spl_token_v2_0::id(),
        &spl_token_v2_0_pubkey(&spl_token_args.token_account_address),
        &spl_token_v2_0_pubkey(&spl_token_args.mint),
        &associated_token_address,
        &spl_token_v2_0_pubkey(&args.sender_keypair.pubkey()),
        &[],
        allocation.amount,
        spl_token_args.decimals,
    )
    .unwrap();
    instructions.push(spl_token_v2_0_instruction(spl_instruction));
    instructions
}

pub fn check_spl_token_balances(
    num_signatures: usize,
    allocations: &[Allocation],
    client: &RpcClient,
    args: &DistributeTokensArgs,
    created_accounts: u64,
) -> Result<(), Error> {
    let spl_token_args = args
        .spl_token_args
        .as_ref()
        .expect("spl_token_args must be some");
    let allocation_amount: u64 = allocations.iter().map(|x| x.amount).sum();

    let fee_calculator = client.get_recent_blockhash()?.1;
    let fees = fee_calculator
        .lamports_per_signature
        .checked_mul(num_signatures as u64)
        .unwrap();

    let token_account_rent_exempt_balance =
        client.get_minimum_balance_for_rent_exemption(SplTokenAccount::LEN)?;
    let account_creation_amount = created_accounts * token_account_rent_exempt_balance;
    let fee_payer_balance = client.get_balance(&args.fee_payer.pubkey())?;
    if fee_payer_balance < fees + account_creation_amount {
        return Err(Error::InsufficientFunds(
            vec![FundingSource::FeePayer].into(),
            lamports_to_sol(fees + account_creation_amount),
        ));
    }
    let source_token_account = client
        .get_account(&spl_token_args.token_account_address)
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

pub fn print_token_balances(
    client: &RpcClient,
    allocation: &Allocation,
    spl_token_args: &SplTokenArgs,
) -> Result<(), Error> {
    let address = allocation.recipient.parse().unwrap();
    let expected = allocation.amount;
    let associated_token_address = get_associated_token_address(
        &spl_token_v2_0_pubkey(&address),
        &spl_token_v2_0_pubkey(&spl_token_args.mint),
    );
    let recipient_account = client
        .get_account(&pubkey_from_spl_token_v2_0(&associated_token_address))
        .unwrap_or_default();
    let (actual, difference) = if let Ok(recipient_token) =
        SplTokenAccount::unpack(&recipient_account.data)
    {
        let actual_ui_amount =
            token_amount_to_ui_amount(recipient_token.amount, spl_token_args.decimals).ui_amount;
        let expected_ui_amount =
            token_amount_to_ui_amount(expected, spl_token_args.decimals).ui_amount;
        (
            style(format!(
                "{:>24.1$}",
                actual_ui_amount, spl_token_args.decimals as usize
            )),
            format!(
                "{:>24.1$}",
                actual_ui_amount - expected_ui_amount,
                spl_token_args.decimals as usize
            ),
        )
    } else {
        (
            style("Associated token account not yet created".to_string()).yellow(),
            "".to_string(),
        )
    };
    println!(
        "{:<44}  {:>24.4$}  {:>24}  {:>24}",
        allocation.recipient,
        token_amount_to_ui_amount(expected, spl_token_args.decimals).ui_amount,
        actual,
        difference,
        spl_token_args.decimals as usize
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    // The following unit tests were written for v1.4 using the ProgramTest framework, passing its
    // BanksClient into the `solana-tokens` methods. With the revert to RpcClient in this module
    // (https://github.com/solana-labs/solana/pull/13623), that approach was no longer viable.
    // These tests were removed rather than rewritten to avoid accruing technical debt. Once a new
    // rpc/client framework is implemented, they should be restored.
    //
    // async fn test_process_spl_token_allocations()
    // async fn test_process_spl_token_transfer_amount_allocations()
    // async fn test_check_spl_token_balances()
    //
    // TODO: link to v1.4 tests
}
