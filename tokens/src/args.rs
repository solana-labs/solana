use solana_sdk::{pubkey::Pubkey, signature::Signer};

pub struct DistributeTokensArgs {
    pub input_csv: String,
    pub from_bids: bool,
    pub transaction_db: String,
    pub dollars_per_sol: Option<f64>,
    pub dry_run: bool,
    pub sender_keypair: Box<dyn Signer>,
    pub fee_payer: Box<dyn Signer>,
    pub stake_args: Option<StakeArgs>,
}

pub struct StakeArgs {
    pub sol_for_fees: f64,
    pub stake_account_address: Pubkey,
    pub stake_authority: Box<dyn Signer>,
    pub withdraw_authority: Box<dyn Signer>,
}

pub struct BalancesArgs {
    pub input_csv: String,
    pub from_bids: bool,
    pub dollars_per_sol: Option<f64>,
}

pub struct TransactionLogArgs {
    pub transaction_db: String,
    pub output_path: String,
}

pub enum Command {
    DistributeTokens(DistributeTokensArgs),
    Balances(BalancesArgs),
    TransactionLog(TransactionLogArgs),
}

pub struct Args {
    pub config_file: String,
    pub url: Option<String>,
    pub command: Command,
}
