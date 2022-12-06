use {
    crate::{
        address_lookup_table::*, clap_app::*, cluster_query::*, feature::*, inflation::*, nonce::*,
        program::*, spend_utils::*, stake::*, validator_info::*, vote::*, wallet::*,
    },
    clap::{crate_description, crate_name, value_t_or_exit, ArgMatches, Shell},
    log::*,
    num_traits::FromPrimitive,
    serde_json::{self, Value},
    solana_clap_utils::{self, input_parsers::*, keypair::*},
    solana_cli_config::ConfigInput,
    solana_cli_output::{
        display::println_name_value, CliSignature, CliValidatorsSortOrder, OutputFormat,
    },
    solana_remote_wallet::remote_wallet::RemoteWalletManager,
    solana_rpc_client::rpc_client::RpcClient,
    solana_rpc_client_api::{
        client_error::{Error as ClientError, Result as ClientResult},
        config::{RpcLargestAccountsFilter, RpcSendTransactionConfig, RpcTransactionLogsFilter},
    },
    solana_rpc_client_nonce_utils::blockhash_query::BlockhashQuery,
    solana_sdk::{
        clock::{Epoch, Slot},
        commitment_config::CommitmentConfig,
        decode_error::DecodeError,
        hash::Hash,
        instruction::InstructionError,
        offchain_message::OffchainMessage,
        pubkey::Pubkey,
        signature::{Signature, Signer, SignerError},
        stake::{instruction::LockupArgs, state::Lockup},
        transaction::{TransactionError, VersionedTransaction},
    },
    solana_tpu_client::tpu_connection_cache::DEFAULT_TPU_ENABLE_UDP,
    solana_vote_program::vote_state::VoteAuthorize,
    std::{collections::HashMap, error, io::stdout, str::FromStr, sync::Arc, time::Duration},
    thiserror::Error,
};

pub const DEFAULT_RPC_TIMEOUT_SECONDS: &str = "30";
pub const DEFAULT_CONFIRM_TX_TIMEOUT_SECONDS: &str = "5";
const CHECKED: bool = true;

#[derive(Debug, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum CliCommand {
    // Cluster Query Commands
    Catchup {
        node_pubkey: Option<Pubkey>,
        node_json_rpc_url: Option<String>,
        follow: bool,
        our_localhost_port: Option<u16>,
        log: bool,
    },
    ClusterDate,
    ClusterVersion,
    Feature(FeatureCliCommand),
    Inflation(InflationCliCommand),
    Fees {
        blockhash: Option<Hash>,
    },
    FirstAvailableBlock,
    GetBlock {
        slot: Option<Slot>,
    },
    GetBlockTime {
        slot: Option<Slot>,
    },
    GetEpoch,
    GetEpochInfo,
    GetGenesisHash,
    GetSlot,
    GetBlockHeight,
    GetTransactionCount,
    LargestAccounts {
        filter: Option<RpcLargestAccountsFilter>,
    },
    LeaderSchedule {
        epoch: Option<Epoch>,
    },
    LiveSlots,
    Logs {
        filter: RpcTransactionLogsFilter,
    },
    Ping {
        interval: Duration,
        count: Option<u64>,
        timeout: Duration,
        blockhash: Option<Hash>,
        print_timestamp: bool,
        compute_unit_price: Option<u64>,
    },
    Rent {
        data_length: usize,
        use_lamports_unit: bool,
    },
    ShowBlockProduction {
        epoch: Option<Epoch>,
        slot_limit: Option<u64>,
    },
    ShowGossip,
    ShowStakes {
        use_lamports_unit: bool,
        vote_account_pubkeys: Option<Vec<Pubkey>>,
        withdraw_authority: Option<Pubkey>,
    },
    ShowValidators {
        use_lamports_unit: bool,
        sort_order: CliValidatorsSortOrder,
        reverse_sort: bool,
        number_validators: bool,
        keep_unstaked_delinquents: bool,
        delinquent_slot_distance: Option<Slot>,
    },
    Supply {
        print_accounts: bool,
    },
    TotalSupply,
    TransactionHistory {
        address: Pubkey,
        before: Option<Signature>,
        until: Option<Signature>,
        limit: usize,
        show_transactions: bool,
    },
    WaitForMaxStake {
        max_stake_percent: f32,
    },
    // Nonce commands
    AuthorizeNonceAccount {
        nonce_account: Pubkey,
        nonce_authority: SignerIndex,
        memo: Option<String>,
        new_authority: Pubkey,
        compute_unit_price: Option<u64>,
    },
    CreateNonceAccount {
        nonce_account: SignerIndex,
        seed: Option<String>,
        nonce_authority: Option<Pubkey>,
        memo: Option<String>,
        amount: SpendAmount,
        compute_unit_price: Option<u64>,
    },
    GetNonce(Pubkey),
    NewNonce {
        nonce_account: Pubkey,
        nonce_authority: SignerIndex,
        memo: Option<String>,
        compute_unit_price: Option<u64>,
    },
    ShowNonceAccount {
        nonce_account_pubkey: Pubkey,
        use_lamports_unit: bool,
    },
    WithdrawFromNonceAccount {
        nonce_account: Pubkey,
        nonce_authority: SignerIndex,
        memo: Option<String>,
        destination_account_pubkey: Pubkey,
        lamports: u64,
        compute_unit_price: Option<u64>,
    },
    UpgradeNonceAccount {
        nonce_account: Pubkey,
        memo: Option<String>,
        compute_unit_price: Option<u64>,
    },
    // Program Deployment
    Deploy,
    Program(ProgramCliCommand),
    // Stake Commands
    CreateStakeAccount {
        stake_account: SignerIndex,
        seed: Option<String>,
        staker: Option<Pubkey>,
        withdrawer: Option<Pubkey>,
        withdrawer_signer: Option<SignerIndex>,
        lockup: Lockup,
        amount: SpendAmount,
        sign_only: bool,
        dump_transaction_message: bool,
        blockhash_query: BlockhashQuery,
        nonce_account: Option<Pubkey>,
        nonce_authority: SignerIndex,
        memo: Option<String>,
        fee_payer: SignerIndex,
        from: SignerIndex,
        compute_unit_price: Option<u64>,
    },
    DeactivateStake {
        stake_account_pubkey: Pubkey,
        stake_authority: SignerIndex,
        sign_only: bool,
        deactivate_delinquent: bool,
        dump_transaction_message: bool,
        blockhash_query: BlockhashQuery,
        nonce_account: Option<Pubkey>,
        nonce_authority: SignerIndex,
        memo: Option<String>,
        seed: Option<String>,
        fee_payer: SignerIndex,
        compute_unit_price: Option<u64>,
    },
    DelegateStake {
        stake_account_pubkey: Pubkey,
        vote_account_pubkey: Pubkey,
        stake_authority: SignerIndex,
        force: bool,
        sign_only: bool,
        dump_transaction_message: bool,
        blockhash_query: BlockhashQuery,
        nonce_account: Option<Pubkey>,
        nonce_authority: SignerIndex,
        memo: Option<String>,
        fee_payer: SignerIndex,
        redelegation_stake_account_pubkey: Option<Pubkey>,
        compute_unit_price: Option<u64>,
    },
    SplitStake {
        stake_account_pubkey: Pubkey,
        stake_authority: SignerIndex,
        sign_only: bool,
        dump_transaction_message: bool,
        blockhash_query: BlockhashQuery,
        nonce_account: Option<Pubkey>,
        nonce_authority: SignerIndex,
        memo: Option<String>,
        split_stake_account: SignerIndex,
        seed: Option<String>,
        lamports: u64,
        fee_payer: SignerIndex,
        compute_unit_price: Option<u64>,
    },
    MergeStake {
        stake_account_pubkey: Pubkey,
        source_stake_account_pubkey: Pubkey,
        stake_authority: SignerIndex,
        sign_only: bool,
        dump_transaction_message: bool,
        blockhash_query: BlockhashQuery,
        nonce_account: Option<Pubkey>,
        nonce_authority: SignerIndex,
        memo: Option<String>,
        fee_payer: SignerIndex,
        compute_unit_price: Option<u64>,
    },
    ShowStakeHistory {
        use_lamports_unit: bool,
        limit_results: usize,
    },
    ShowStakeAccount {
        pubkey: Pubkey,
        use_lamports_unit: bool,
        with_rewards: Option<usize>,
    },
    StakeAuthorize {
        stake_account_pubkey: Pubkey,
        new_authorizations: Vec<StakeAuthorizationIndexed>,
        sign_only: bool,
        dump_transaction_message: bool,
        blockhash_query: BlockhashQuery,
        nonce_account: Option<Pubkey>,
        nonce_authority: SignerIndex,
        memo: Option<String>,
        fee_payer: SignerIndex,
        custodian: Option<SignerIndex>,
        no_wait: bool,
        compute_unit_price: Option<u64>,
    },
    StakeSetLockup {
        stake_account_pubkey: Pubkey,
        lockup: LockupArgs,
        custodian: SignerIndex,
        new_custodian_signer: Option<SignerIndex>,
        sign_only: bool,
        dump_transaction_message: bool,
        blockhash_query: BlockhashQuery,
        nonce_account: Option<Pubkey>,
        nonce_authority: SignerIndex,
        memo: Option<String>,
        fee_payer: SignerIndex,
        compute_unit_price: Option<u64>,
    },
    WithdrawStake {
        stake_account_pubkey: Pubkey,
        destination_account_pubkey: Pubkey,
        amount: SpendAmount,
        withdraw_authority: SignerIndex,
        custodian: Option<SignerIndex>,
        sign_only: bool,
        dump_transaction_message: bool,
        blockhash_query: BlockhashQuery,
        nonce_account: Option<Pubkey>,
        nonce_authority: SignerIndex,
        memo: Option<String>,
        seed: Option<String>,
        fee_payer: SignerIndex,
        compute_unit_price: Option<u64>,
    },
    // Validator Info Commands
    GetValidatorInfo(Option<Pubkey>),
    SetValidatorInfo {
        validator_info: Value,
        force_keybase: bool,
        info_pubkey: Option<Pubkey>,
    },
    // Vote Commands
    CreateVoteAccount {
        vote_account: SignerIndex,
        seed: Option<String>,
        identity_account: SignerIndex,
        authorized_voter: Option<Pubkey>,
        authorized_withdrawer: Pubkey,
        commission: u8,
        sign_only: bool,
        dump_transaction_message: bool,
        blockhash_query: BlockhashQuery,
        nonce_account: Option<Pubkey>,
        nonce_authority: SignerIndex,
        memo: Option<String>,
        fee_payer: SignerIndex,
        compute_unit_price: Option<u64>,
    },
    ShowVoteAccount {
        pubkey: Pubkey,
        use_lamports_unit: bool,
        with_rewards: Option<usize>,
    },
    WithdrawFromVoteAccount {
        vote_account_pubkey: Pubkey,
        destination_account_pubkey: Pubkey,
        withdraw_authority: SignerIndex,
        withdraw_amount: SpendAmount,
        sign_only: bool,
        dump_transaction_message: bool,
        blockhash_query: BlockhashQuery,
        nonce_account: Option<Pubkey>,
        nonce_authority: SignerIndex,
        memo: Option<String>,
        fee_payer: SignerIndex,
        compute_unit_price: Option<u64>,
    },
    CloseVoteAccount {
        vote_account_pubkey: Pubkey,
        destination_account_pubkey: Pubkey,
        withdraw_authority: SignerIndex,
        memo: Option<String>,
        fee_payer: SignerIndex,
        compute_unit_price: Option<u64>,
    },
    VoteAuthorize {
        vote_account_pubkey: Pubkey,
        new_authorized_pubkey: Pubkey,
        vote_authorize: VoteAuthorize,
        sign_only: bool,
        dump_transaction_message: bool,
        blockhash_query: BlockhashQuery,
        nonce_account: Option<Pubkey>,
        nonce_authority: SignerIndex,
        memo: Option<String>,
        fee_payer: SignerIndex,
        authorized: SignerIndex,
        new_authorized: Option<SignerIndex>,
        compute_unit_price: Option<u64>,
    },
    VoteUpdateValidator {
        vote_account_pubkey: Pubkey,
        new_identity_account: SignerIndex,
        withdraw_authority: SignerIndex,
        sign_only: bool,
        dump_transaction_message: bool,
        blockhash_query: BlockhashQuery,
        nonce_account: Option<Pubkey>,
        nonce_authority: SignerIndex,
        memo: Option<String>,
        fee_payer: SignerIndex,
        compute_unit_price: Option<u64>,
    },
    VoteUpdateCommission {
        vote_account_pubkey: Pubkey,
        commission: u8,
        withdraw_authority: SignerIndex,
        sign_only: bool,
        dump_transaction_message: bool,
        blockhash_query: BlockhashQuery,
        nonce_account: Option<Pubkey>,
        nonce_authority: SignerIndex,
        memo: Option<String>,
        fee_payer: SignerIndex,
        compute_unit_price: Option<u64>,
    },
    // Wallet Commands
    Address,
    Airdrop {
        pubkey: Option<Pubkey>,
        lamports: u64,
    },
    Balance {
        pubkey: Option<Pubkey>,
        use_lamports_unit: bool,
    },
    Confirm(Signature),
    CreateAddressWithSeed {
        from_pubkey: Option<Pubkey>,
        seed: String,
        program_id: Pubkey,
    },
    DecodeTransaction(VersionedTransaction),
    ResolveSigner(Option<String>),
    ShowAccount {
        pubkey: Pubkey,
        output_file: Option<String>,
        use_lamports_unit: bool,
    },
    Transfer {
        amount: SpendAmount,
        to: Pubkey,
        from: SignerIndex,
        sign_only: bool,
        dump_transaction_message: bool,
        allow_unfunded_recipient: bool,
        no_wait: bool,
        blockhash_query: BlockhashQuery,
        nonce_account: Option<Pubkey>,
        nonce_authority: SignerIndex,
        memo: Option<String>,
        fee_payer: SignerIndex,
        derived_address_seed: Option<String>,
        derived_address_program_id: Option<Pubkey>,
        compute_unit_price: Option<u64>,
    },
    StakeMinimumDelegation {
        use_lamports_unit: bool,
    },
    // Address lookup table commands
    AddressLookupTable(AddressLookupTableCliCommand),
    SignOffchainMessage {
        message: OffchainMessage,
    },
    VerifyOffchainSignature {
        signer_pubkey: Option<Pubkey>,
        signature: Signature,
        message: OffchainMessage,
    },
}

#[derive(Debug, PartialEq)]
pub struct CliCommandInfo {
    pub command: CliCommand,
    pub signers: CliSigners,
}

#[derive(Debug, Error)]
pub enum CliError {
    #[error("Bad parameter: {0}")]
    BadParameter(String),
    #[error(transparent)]
    ClientError(#[from] ClientError),
    #[error("Command not recognized: {0}")]
    CommandNotRecognized(String),
    #[error("Account {1} has insufficient funds for fee ({0} SOL)")]
    InsufficientFundsForFee(f64, Pubkey),
    #[error("Account {1} has insufficient funds for spend ({0} SOL)")]
    InsufficientFundsForSpend(f64, Pubkey),
    #[error("Account {2} has insufficient funds for spend ({0} SOL) + fee ({1} SOL)")]
    InsufficientFundsForSpendAndFee(f64, f64, Pubkey),
    #[error(transparent)]
    InvalidNonce(solana_rpc_client_nonce_utils::Error),
    #[error("Dynamic program error: {0}")]
    DynamicProgramError(String),
    #[error("RPC request error: {0}")]
    RpcRequestError(String),
    #[error("Keypair file not found: {0}")]
    KeypairFileNotFound(String),
    #[error("Invalid signature")]
    InvalidSignature,
}

impl From<Box<dyn error::Error>> for CliError {
    fn from(error: Box<dyn error::Error>) -> Self {
        CliError::DynamicProgramError(error.to_string())
    }
}

impl From<solana_rpc_client_nonce_utils::Error> for CliError {
    fn from(error: solana_rpc_client_nonce_utils::Error) -> Self {
        match error {
            solana_rpc_client_nonce_utils::Error::Client(client_error) => {
                Self::RpcRequestError(client_error)
            }
            _ => Self::InvalidNonce(error),
        }
    }
}

pub struct CliConfig<'a> {
    pub command: CliCommand,
    pub json_rpc_url: String,
    pub websocket_url: String,
    pub keypair_path: String,
    pub commitment: CommitmentConfig,
    pub signers: Vec<&'a dyn Signer>,
    pub rpc_client: Option<Arc<RpcClient>>,
    pub rpc_timeout: Duration,
    pub verbose: bool,
    pub output_format: OutputFormat,
    pub send_transaction_config: RpcSendTransactionConfig,
    pub confirm_transaction_initial_timeout: Duration,
    pub address_labels: HashMap<String, String>,
    pub use_quic: bool,
}

impl CliConfig<'_> {
    pub(crate) fn pubkey(&self) -> Result<Pubkey, SignerError> {
        if !self.signers.is_empty() {
            self.signers[0].try_pubkey()
        } else {
            Err(SignerError::Custom(
                "Default keypair must be set if pubkey arg not provided".to_string(),
            ))
        }
    }

    pub fn recent_for_tests() -> Self {
        Self {
            commitment: CommitmentConfig::processed(),
            send_transaction_config: RpcSendTransactionConfig {
                skip_preflight: true,
                preflight_commitment: Some(CommitmentConfig::processed().commitment),
                ..RpcSendTransactionConfig::default()
            },
            ..Self::default()
        }
    }
}

impl Default for CliConfig<'_> {
    fn default() -> CliConfig<'static> {
        CliConfig {
            command: CliCommand::Balance {
                pubkey: Some(Pubkey::default()),
                use_lamports_unit: false,
            },
            json_rpc_url: ConfigInput::default().json_rpc_url,
            websocket_url: ConfigInput::default().websocket_url,
            keypair_path: ConfigInput::default().keypair_path,
            commitment: ConfigInput::default().commitment,
            signers: Vec::new(),
            rpc_client: None,
            rpc_timeout: Duration::from_secs(u64::from_str(DEFAULT_RPC_TIMEOUT_SECONDS).unwrap()),
            verbose: false,
            output_format: OutputFormat::Display,
            send_transaction_config: RpcSendTransactionConfig::default(),
            confirm_transaction_initial_timeout: Duration::from_secs(
                u64::from_str(DEFAULT_CONFIRM_TX_TIMEOUT_SECONDS).unwrap(),
            ),
            address_labels: HashMap::new(),
            use_quic: !DEFAULT_TPU_ENABLE_UDP,
        }
    }
}

pub fn parse_command(
    matches: &ArgMatches<'_>,
    default_signer: &DefaultSigner,
    wallet_manager: &mut Option<Arc<RemoteWalletManager>>,
) -> Result<CliCommandInfo, Box<dyn error::Error>> {
    let response = match matches.subcommand() {
        // Autocompletion Command
        ("completion", Some(matches)) => {
            let shell_choice = match matches.value_of("shell") {
                Some("bash") => Shell::Bash,
                Some("fish") => Shell::Fish,
                Some("zsh") => Shell::Zsh,
                Some("powershell") => Shell::PowerShell,
                Some("elvish") => Shell::Elvish,
                // This is safe, since we assign default_value and possible_values
                // are restricted
                _ => unreachable!(),
            };
            get_clap_app(
                crate_name!(),
                crate_description!(),
                solana_version::version!(),
            )
            .gen_completions_to("solana", shell_choice, &mut stdout());
            std::process::exit(0);
        }
        // Cluster Query Commands
        ("block", Some(matches)) => parse_get_block(matches),
        ("block-height", Some(matches)) => parse_get_block_height(matches),
        ("block-production", Some(matches)) => parse_show_block_production(matches),
        ("block-time", Some(matches)) => parse_get_block_time(matches),
        ("catchup", Some(matches)) => parse_catchup(matches, wallet_manager),
        ("cluster-date", Some(_matches)) => Ok(CliCommandInfo {
            command: CliCommand::ClusterDate,
            signers: vec![],
        }),
        ("cluster-version", Some(_matches)) => Ok(CliCommandInfo {
            command: CliCommand::ClusterVersion,
            signers: vec![],
        }),
        ("epoch", Some(matches)) => parse_get_epoch(matches),
        ("epoch-info", Some(matches)) => parse_get_epoch_info(matches),
        ("feature", Some(matches)) => {
            parse_feature_subcommand(matches, default_signer, wallet_manager)
        }
        ("fees", Some(matches)) => {
            let blockhash = value_of::<Hash>(matches, "blockhash");
            Ok(CliCommandInfo {
                command: CliCommand::Fees { blockhash },
                signers: vec![],
            })
        }
        ("first-available-block", Some(_matches)) => Ok(CliCommandInfo {
            command: CliCommand::FirstAvailableBlock,
            signers: vec![],
        }),
        ("genesis-hash", Some(_matches)) => Ok(CliCommandInfo {
            command: CliCommand::GetGenesisHash,
            signers: vec![],
        }),
        ("gossip", Some(_matches)) => Ok(CliCommandInfo {
            command: CliCommand::ShowGossip,
            signers: vec![],
        }),
        ("inflation", Some(matches)) => {
            parse_inflation_subcommand(matches, default_signer, wallet_manager)
        }
        ("largest-accounts", Some(matches)) => parse_largest_accounts(matches),
        ("leader-schedule", Some(matches)) => parse_leader_schedule(matches),
        ("live-slots", Some(_matches)) => Ok(CliCommandInfo {
            command: CliCommand::LiveSlots,
            signers: vec![],
        }),
        ("logs", Some(matches)) => parse_logs(matches, wallet_manager),
        ("ping", Some(matches)) => parse_cluster_ping(matches, default_signer, wallet_manager),
        ("rent", Some(matches)) => {
            let data_length = value_of::<RentLengthValue>(matches, "data_length")
                .unwrap()
                .length();
            let use_lamports_unit = matches.is_present("lamports");
            Ok(CliCommandInfo {
                command: CliCommand::Rent {
                    data_length,
                    use_lamports_unit,
                },
                signers: vec![],
            })
        }
        ("slot", Some(matches)) => parse_get_slot(matches),
        ("stakes", Some(matches)) => parse_show_stakes(matches, wallet_manager),
        ("supply", Some(matches)) => parse_supply(matches),
        ("total-supply", Some(matches)) => parse_total_supply(matches),
        ("transaction-count", Some(matches)) => parse_get_transaction_count(matches),
        ("transaction-history", Some(matches)) => {
            parse_transaction_history(matches, wallet_manager)
        }
        ("validators", Some(matches)) => parse_show_validators(matches),
        // Nonce Commands
        ("authorize-nonce-account", Some(matches)) => {
            parse_authorize_nonce_account(matches, default_signer, wallet_manager)
        }
        ("create-nonce-account", Some(matches)) => {
            parse_nonce_create_account(matches, default_signer, wallet_manager)
        }
        ("nonce", Some(matches)) => parse_get_nonce(matches, wallet_manager),
        ("new-nonce", Some(matches)) => parse_new_nonce(matches, default_signer, wallet_manager),
        ("nonce-account", Some(matches)) => parse_show_nonce_account(matches, wallet_manager),
        ("withdraw-from-nonce-account", Some(matches)) => {
            parse_withdraw_from_nonce_account(matches, default_signer, wallet_manager)
        }
        ("upgrade-nonce-account", Some(matches)) => parse_upgrade_nonce_account(matches),
        // Program Deployment
        ("deploy", Some(_matches)) => clap::Error::with_description(
            "`solana deploy` has been replaced with `solana program deploy`",
            clap::ErrorKind::UnrecognizedSubcommand,
        )
        .exit(),
        ("program", Some(matches)) => {
            parse_program_subcommand(matches, default_signer, wallet_manager)
        }
        ("address-lookup-table", Some(matches)) => {
            parse_address_lookup_table_subcommand(matches, default_signer, wallet_manager)
        }
        ("wait-for-max-stake", Some(matches)) => {
            let max_stake_percent = value_t_or_exit!(matches, "max_percent", f32);
            Ok(CliCommandInfo {
                command: CliCommand::WaitForMaxStake { max_stake_percent },
                signers: vec![],
            })
        }
        // Stake Commands
        ("create-stake-account", Some(matches)) => {
            parse_create_stake_account(matches, default_signer, wallet_manager, !CHECKED)
        }
        ("create-stake-account-checked", Some(matches)) => {
            parse_create_stake_account(matches, default_signer, wallet_manager, CHECKED)
        }
        ("delegate-stake", Some(matches)) => {
            parse_stake_delegate_stake(matches, default_signer, wallet_manager)
        }
        ("redelegate-stake", Some(matches)) => {
            parse_stake_delegate_stake(matches, default_signer, wallet_manager)
        }
        ("withdraw-stake", Some(matches)) => {
            parse_stake_withdraw_stake(matches, default_signer, wallet_manager)
        }
        ("deactivate-stake", Some(matches)) => {
            parse_stake_deactivate_stake(matches, default_signer, wallet_manager)
        }
        ("split-stake", Some(matches)) => {
            parse_split_stake(matches, default_signer, wallet_manager)
        }
        ("merge-stake", Some(matches)) => {
            parse_merge_stake(matches, default_signer, wallet_manager)
        }
        ("stake-authorize", Some(matches)) => {
            parse_stake_authorize(matches, default_signer, wallet_manager, !CHECKED)
        }
        ("stake-authorize-checked", Some(matches)) => {
            parse_stake_authorize(matches, default_signer, wallet_manager, CHECKED)
        }
        ("stake-set-lockup", Some(matches)) => {
            parse_stake_set_lockup(matches, default_signer, wallet_manager, !CHECKED)
        }
        ("stake-set-lockup-checked", Some(matches)) => {
            parse_stake_set_lockup(matches, default_signer, wallet_manager, CHECKED)
        }
        ("stake-account", Some(matches)) => parse_show_stake_account(matches, wallet_manager),
        ("stake-history", Some(matches)) => parse_show_stake_history(matches),
        ("stake-minimum-delegation", Some(matches)) => parse_stake_minimum_delegation(matches),
        // Validator Info Commands
        ("validator-info", Some(matches)) => match matches.subcommand() {
            ("publish", Some(matches)) => {
                parse_validator_info_command(matches, default_signer, wallet_manager)
            }
            ("get", Some(matches)) => parse_get_validator_info_command(matches),
            _ => unreachable!(),
        },
        // Vote Commands
        ("create-vote-account", Some(matches)) => {
            parse_create_vote_account(matches, default_signer, wallet_manager)
        }
        ("vote-update-validator", Some(matches)) => {
            parse_vote_update_validator(matches, default_signer, wallet_manager)
        }
        ("vote-update-commission", Some(matches)) => {
            parse_vote_update_commission(matches, default_signer, wallet_manager)
        }
        ("vote-authorize-voter", Some(matches)) => parse_vote_authorize(
            matches,
            default_signer,
            wallet_manager,
            VoteAuthorize::Voter,
            !CHECKED,
        ),
        ("vote-authorize-withdrawer", Some(matches)) => parse_vote_authorize(
            matches,
            default_signer,
            wallet_manager,
            VoteAuthorize::Withdrawer,
            !CHECKED,
        ),
        ("vote-authorize-voter-checked", Some(matches)) => parse_vote_authorize(
            matches,
            default_signer,
            wallet_manager,
            VoteAuthorize::Voter,
            CHECKED,
        ),
        ("vote-authorize-withdrawer-checked", Some(matches)) => parse_vote_authorize(
            matches,
            default_signer,
            wallet_manager,
            VoteAuthorize::Withdrawer,
            CHECKED,
        ),
        ("vote-account", Some(matches)) => parse_vote_get_account_command(matches, wallet_manager),
        ("withdraw-from-vote-account", Some(matches)) => {
            parse_withdraw_from_vote_account(matches, default_signer, wallet_manager)
        }
        ("close-vote-account", Some(matches)) => {
            parse_close_vote_account(matches, default_signer, wallet_manager)
        }
        // Wallet Commands
        ("account", Some(matches)) => parse_account(matches, wallet_manager),
        ("address", Some(matches)) => Ok(CliCommandInfo {
            command: CliCommand::Address,
            signers: vec![default_signer.signer_from_path(matches, wallet_manager)?],
        }),
        ("airdrop", Some(matches)) => parse_airdrop(matches, default_signer, wallet_manager),
        ("balance", Some(matches)) => parse_balance(matches, default_signer, wallet_manager),
        ("confirm", Some(matches)) => match matches.value_of("signature").unwrap().parse() {
            Ok(signature) => Ok(CliCommandInfo {
                command: CliCommand::Confirm(signature),
                signers: vec![],
            }),
            _ => Err(CliError::BadParameter("Invalid signature".to_string())),
        },
        ("create-address-with-seed", Some(matches)) => {
            parse_create_address_with_seed(matches, default_signer, wallet_manager)
        }
        ("decode-transaction", Some(matches)) => parse_decode_transaction(matches),
        ("resolve-signer", Some(matches)) => {
            let signer_path = resolve_signer(matches, "signer", wallet_manager)?;
            Ok(CliCommandInfo {
                command: CliCommand::ResolveSigner(signer_path),
                signers: vec![],
            })
        }
        ("transfer", Some(matches)) => parse_transfer(matches, default_signer, wallet_manager),
        ("sign-offchain-message", Some(matches)) => {
            parse_sign_offchain_message(matches, default_signer, wallet_manager)
        }
        ("verify-offchain-signature", Some(matches)) => {
            parse_verify_offchain_signature(matches, default_signer, wallet_manager)
        }
        //
        ("", None) => {
            eprintln!("{}", matches.usage());
            Err(CliError::CommandNotRecognized(
                "no subcommand given".to_string(),
            ))
        }
        _ => unreachable!(),
    }?;
    Ok(response)
}

pub type ProcessResult = Result<String, Box<dyn std::error::Error>>;

pub fn process_command(config: &CliConfig) -> ProcessResult {
    if config.verbose && config.output_format == OutputFormat::DisplayVerbose {
        println_name_value("RPC URL:", &config.json_rpc_url);
        println_name_value("Default Signer Path:", &config.keypair_path);
        if config.keypair_path.starts_with("usb://") {
            let pubkey = config
                .pubkey()
                .map(|pubkey| format!("{pubkey:?}"))
                .unwrap_or_else(|_| "Unavailable".to_string());
            println_name_value("Pubkey:", &pubkey);
        }
        println_name_value("Commitment:", &config.commitment.commitment.to_string());
    }

    let rpc_client = if config.rpc_client.is_none() {
        Arc::new(RpcClient::new_with_timeouts_and_commitment(
            config.json_rpc_url.to_string(),
            config.rpc_timeout,
            config.commitment,
            config.confirm_transaction_initial_timeout,
        ))
    } else {
        // Primarily for testing
        config.rpc_client.as_ref().unwrap().clone()
    };

    match &config.command {
        // Cluster Query Commands
        // Get address of this client
        CliCommand::Address => Ok(format!("{}", config.pubkey()?)),
        // Return software version of solana-cli and cluster entrypoint node
        CliCommand::Catchup {
            node_pubkey,
            node_json_rpc_url,
            follow,
            our_localhost_port,
            log,
        } => process_catchup(
            &rpc_client,
            config,
            *node_pubkey,
            node_json_rpc_url.clone(),
            *follow,
            *our_localhost_port,
            *log,
        ),
        CliCommand::ClusterDate => process_cluster_date(&rpc_client, config),
        CliCommand::ClusterVersion => process_cluster_version(&rpc_client, config),
        CliCommand::CreateAddressWithSeed {
            from_pubkey,
            seed,
            program_id,
        } => process_create_address_with_seed(config, from_pubkey.as_ref(), seed, program_id),
        CliCommand::Fees { ref blockhash } => process_fees(&rpc_client, config, blockhash.as_ref()),
        CliCommand::Feature(feature_subcommand) => {
            process_feature_subcommand(&rpc_client, config, feature_subcommand)
        }
        CliCommand::FirstAvailableBlock => process_first_available_block(&rpc_client),
        CliCommand::GetBlock { slot } => process_get_block(&rpc_client, config, *slot),
        CliCommand::GetBlockTime { slot } => process_get_block_time(&rpc_client, config, *slot),
        CliCommand::GetEpoch => process_get_epoch(&rpc_client, config),
        CliCommand::GetEpochInfo => process_get_epoch_info(&rpc_client, config),
        CliCommand::GetGenesisHash => process_get_genesis_hash(&rpc_client),
        CliCommand::GetSlot => process_get_slot(&rpc_client, config),
        CliCommand::GetBlockHeight => process_get_block_height(&rpc_client, config),
        CliCommand::LargestAccounts { filter } => {
            process_largest_accounts(&rpc_client, config, filter.clone())
        }
        CliCommand::GetTransactionCount => process_get_transaction_count(&rpc_client, config),
        CliCommand::Inflation(inflation_subcommand) => {
            process_inflation_subcommand(&rpc_client, config, inflation_subcommand)
        }
        CliCommand::LeaderSchedule { epoch } => {
            process_leader_schedule(&rpc_client, config, *epoch)
        }
        CliCommand::LiveSlots => process_live_slots(config),
        CliCommand::Logs { filter } => process_logs(config, filter),
        CliCommand::Ping {
            interval,
            count,
            timeout,
            blockhash,
            print_timestamp,
            compute_unit_price,
        } => process_ping(
            &rpc_client,
            config,
            interval,
            count,
            timeout,
            blockhash,
            *print_timestamp,
            compute_unit_price.as_ref(),
        ),
        CliCommand::Rent {
            data_length,
            use_lamports_unit,
        } => process_calculate_rent(&rpc_client, config, *data_length, *use_lamports_unit),
        CliCommand::ShowBlockProduction { epoch, slot_limit } => {
            process_show_block_production(&rpc_client, config, *epoch, *slot_limit)
        }
        CliCommand::ShowGossip => process_show_gossip(&rpc_client, config),
        CliCommand::ShowStakes {
            use_lamports_unit,
            vote_account_pubkeys,
            withdraw_authority,
        } => process_show_stakes(
            &rpc_client,
            config,
            *use_lamports_unit,
            vote_account_pubkeys.as_deref(),
            withdraw_authority.as_ref(),
        ),
        CliCommand::WaitForMaxStake { max_stake_percent } => {
            process_wait_for_max_stake(&rpc_client, config, *max_stake_percent)
        }
        CliCommand::ShowValidators {
            use_lamports_unit,
            sort_order,
            reverse_sort,
            number_validators,
            keep_unstaked_delinquents,
            delinquent_slot_distance,
        } => process_show_validators(
            &rpc_client,
            config,
            *use_lamports_unit,
            *sort_order,
            *reverse_sort,
            *number_validators,
            *keep_unstaked_delinquents,
            *delinquent_slot_distance,
        ),
        CliCommand::Supply { print_accounts } => {
            process_supply(&rpc_client, config, *print_accounts)
        }
        CliCommand::TotalSupply => process_total_supply(&rpc_client, config),
        CliCommand::TransactionHistory {
            address,
            before,
            until,
            limit,
            show_transactions,
        } => process_transaction_history(
            &rpc_client,
            config,
            address,
            *before,
            *until,
            *limit,
            *show_transactions,
        ),

        // Nonce Commands

        // Assign authority to nonce account
        CliCommand::AuthorizeNonceAccount {
            nonce_account,
            nonce_authority,
            memo,
            new_authority,
            compute_unit_price,
        } => process_authorize_nonce_account(
            &rpc_client,
            config,
            nonce_account,
            *nonce_authority,
            memo.as_ref(),
            new_authority,
            compute_unit_price.as_ref(),
        ),
        // Create nonce account
        CliCommand::CreateNonceAccount {
            nonce_account,
            seed,
            nonce_authority,
            memo,
            amount,
            compute_unit_price,
        } => process_create_nonce_account(
            &rpc_client,
            config,
            *nonce_account,
            seed.clone(),
            *nonce_authority,
            memo.as_ref(),
            *amount,
            compute_unit_price.as_ref(),
        ),
        // Get the current nonce
        CliCommand::GetNonce(nonce_account_pubkey) => {
            process_get_nonce(&rpc_client, config, nonce_account_pubkey)
        }
        // Get a new nonce
        CliCommand::NewNonce {
            nonce_account,
            nonce_authority,
            memo,
            compute_unit_price,
        } => process_new_nonce(
            &rpc_client,
            config,
            nonce_account,
            *nonce_authority,
            memo.as_ref(),
            compute_unit_price.as_ref(),
        ),
        // Show the contents of a nonce account
        CliCommand::ShowNonceAccount {
            nonce_account_pubkey,
            use_lamports_unit,
        } => process_show_nonce_account(
            &rpc_client,
            config,
            nonce_account_pubkey,
            *use_lamports_unit,
        ),
        // Withdraw lamports from a nonce account
        CliCommand::WithdrawFromNonceAccount {
            nonce_account,
            nonce_authority,
            memo,
            destination_account_pubkey,
            lamports,
            compute_unit_price,
        } => process_withdraw_from_nonce_account(
            &rpc_client,
            config,
            nonce_account,
            *nonce_authority,
            memo.as_ref(),
            destination_account_pubkey,
            *lamports,
            compute_unit_price.as_ref(),
        ),
        // Upgrade nonce account out of blockhash domain.
        CliCommand::UpgradeNonceAccount {
            nonce_account,
            memo,
            compute_unit_price,
        } => process_upgrade_nonce_account(
            &rpc_client,
            config,
            *nonce_account,
            memo.as_ref(),
            compute_unit_price.as_ref(),
        ),

        // Program Deployment
        CliCommand::Deploy => {
            // This command is not supported any longer
            // Error message is printed on the previous stage
            std::process::exit(1);
        }

        // Deploy a custom program to the chain
        CliCommand::Program(program_subcommand) => {
            process_program_subcommand(rpc_client, config, program_subcommand)
        }

        // Stake Commands

        // Create stake account
        CliCommand::CreateStakeAccount {
            stake_account,
            seed,
            staker,
            withdrawer,
            withdrawer_signer,
            lockup,
            amount,
            sign_only,
            dump_transaction_message,
            blockhash_query,
            ref nonce_account,
            nonce_authority,
            memo,
            fee_payer,
            from,
            compute_unit_price,
        } => process_create_stake_account(
            &rpc_client,
            config,
            *stake_account,
            seed,
            staker,
            withdrawer,
            *withdrawer_signer,
            lockup,
            *amount,
            *sign_only,
            *dump_transaction_message,
            blockhash_query,
            nonce_account.as_ref(),
            *nonce_authority,
            memo.as_ref(),
            *fee_payer,
            *from,
            compute_unit_price.as_ref(),
        ),
        CliCommand::DeactivateStake {
            stake_account_pubkey,
            stake_authority,
            sign_only,
            deactivate_delinquent,
            dump_transaction_message,
            blockhash_query,
            nonce_account,
            nonce_authority,
            memo,
            seed,
            fee_payer,
            compute_unit_price,
        } => process_deactivate_stake_account(
            &rpc_client,
            config,
            stake_account_pubkey,
            *stake_authority,
            *sign_only,
            *deactivate_delinquent,
            *dump_transaction_message,
            blockhash_query,
            *nonce_account,
            *nonce_authority,
            memo.as_ref(),
            seed.as_ref(),
            *fee_payer,
            compute_unit_price.as_ref(),
        ),
        CliCommand::DelegateStake {
            stake_account_pubkey,
            vote_account_pubkey,
            stake_authority,
            force,
            sign_only,
            dump_transaction_message,
            blockhash_query,
            nonce_account,
            nonce_authority,
            memo,
            fee_payer,
            redelegation_stake_account_pubkey,
            compute_unit_price,
        } => process_delegate_stake(
            &rpc_client,
            config,
            stake_account_pubkey,
            vote_account_pubkey,
            *stake_authority,
            *force,
            *sign_only,
            *dump_transaction_message,
            blockhash_query,
            *nonce_account,
            *nonce_authority,
            memo.as_ref(),
            *fee_payer,
            redelegation_stake_account_pubkey.as_ref(),
            compute_unit_price.as_ref(),
        ),
        CliCommand::SplitStake {
            stake_account_pubkey,
            stake_authority,
            sign_only,
            dump_transaction_message,
            blockhash_query,
            nonce_account,
            nonce_authority,
            memo,
            split_stake_account,
            seed,
            lamports,
            fee_payer,
            compute_unit_price,
        } => process_split_stake(
            &rpc_client,
            config,
            stake_account_pubkey,
            *stake_authority,
            *sign_only,
            *dump_transaction_message,
            blockhash_query,
            *nonce_account,
            *nonce_authority,
            memo.as_ref(),
            *split_stake_account,
            seed,
            *lamports,
            *fee_payer,
            compute_unit_price.as_ref(),
        ),
        CliCommand::MergeStake {
            stake_account_pubkey,
            source_stake_account_pubkey,
            stake_authority,
            sign_only,
            dump_transaction_message,
            blockhash_query,
            nonce_account,
            nonce_authority,
            memo,
            fee_payer,
            compute_unit_price,
        } => process_merge_stake(
            &rpc_client,
            config,
            stake_account_pubkey,
            source_stake_account_pubkey,
            *stake_authority,
            *sign_only,
            *dump_transaction_message,
            blockhash_query,
            *nonce_account,
            *nonce_authority,
            memo.as_ref(),
            *fee_payer,
            compute_unit_price.as_ref(),
        ),
        CliCommand::ShowStakeAccount {
            pubkey: stake_account_pubkey,
            use_lamports_unit,
            with_rewards,
        } => process_show_stake_account(
            &rpc_client,
            config,
            stake_account_pubkey,
            *use_lamports_unit,
            *with_rewards,
        ),
        CliCommand::ShowStakeHistory {
            use_lamports_unit,
            limit_results,
        } => process_show_stake_history(&rpc_client, config, *use_lamports_unit, *limit_results),
        CliCommand::StakeAuthorize {
            stake_account_pubkey,
            ref new_authorizations,
            sign_only,
            dump_transaction_message,
            blockhash_query,
            nonce_account,
            nonce_authority,
            memo,
            fee_payer,
            custodian,
            no_wait,
            compute_unit_price,
        } => process_stake_authorize(
            &rpc_client,
            config,
            stake_account_pubkey,
            new_authorizations,
            *custodian,
            *sign_only,
            *dump_transaction_message,
            blockhash_query,
            *nonce_account,
            *nonce_authority,
            memo.as_ref(),
            *fee_payer,
            *no_wait,
            compute_unit_price.as_ref(),
        ),
        CliCommand::StakeSetLockup {
            stake_account_pubkey,
            lockup,
            custodian,
            new_custodian_signer,
            sign_only,
            dump_transaction_message,
            blockhash_query,
            nonce_account,
            nonce_authority,
            memo,
            fee_payer,
            compute_unit_price,
        } => process_stake_set_lockup(
            &rpc_client,
            config,
            stake_account_pubkey,
            lockup,
            *new_custodian_signer,
            *custodian,
            *sign_only,
            *dump_transaction_message,
            blockhash_query,
            *nonce_account,
            *nonce_authority,
            memo.as_ref(),
            *fee_payer,
            compute_unit_price.as_ref(),
        ),
        CliCommand::WithdrawStake {
            stake_account_pubkey,
            destination_account_pubkey,
            amount,
            withdraw_authority,
            custodian,
            sign_only,
            dump_transaction_message,
            blockhash_query,
            ref nonce_account,
            nonce_authority,
            memo,
            seed,
            fee_payer,
            compute_unit_price,
        } => process_withdraw_stake(
            &rpc_client,
            config,
            stake_account_pubkey,
            destination_account_pubkey,
            *amount,
            *withdraw_authority,
            *custodian,
            *sign_only,
            *dump_transaction_message,
            blockhash_query,
            nonce_account.as_ref(),
            *nonce_authority,
            memo.as_ref(),
            seed.as_ref(),
            *fee_payer,
            compute_unit_price.as_ref(),
        ),
        CliCommand::StakeMinimumDelegation { use_lamports_unit } => {
            process_stake_minimum_delegation(&rpc_client, config, *use_lamports_unit)
        }

        // Validator Info Commands

        // Return all or single validator info
        CliCommand::GetValidatorInfo(info_pubkey) => {
            process_get_validator_info(&rpc_client, config, *info_pubkey)
        }
        // Publish validator info
        CliCommand::SetValidatorInfo {
            validator_info,
            force_keybase,
            info_pubkey,
        } => process_set_validator_info(
            &rpc_client,
            config,
            validator_info,
            *force_keybase,
            *info_pubkey,
        ),

        // Vote Commands

        // Create vote account
        CliCommand::CreateVoteAccount {
            vote_account,
            seed,
            identity_account,
            authorized_voter,
            authorized_withdrawer,
            commission,
            sign_only,
            dump_transaction_message,
            blockhash_query,
            ref nonce_account,
            nonce_authority,
            memo,
            fee_payer,
            compute_unit_price,
        } => process_create_vote_account(
            &rpc_client,
            config,
            *vote_account,
            seed,
            *identity_account,
            authorized_voter,
            *authorized_withdrawer,
            *commission,
            *sign_only,
            *dump_transaction_message,
            blockhash_query,
            nonce_account.as_ref(),
            *nonce_authority,
            memo.as_ref(),
            *fee_payer,
            compute_unit_price.as_ref(),
        ),
        CliCommand::ShowVoteAccount {
            pubkey: vote_account_pubkey,
            use_lamports_unit,
            with_rewards,
        } => process_show_vote_account(
            &rpc_client,
            config,
            vote_account_pubkey,
            *use_lamports_unit,
            *with_rewards,
        ),
        CliCommand::WithdrawFromVoteAccount {
            vote_account_pubkey,
            withdraw_authority,
            withdraw_amount,
            destination_account_pubkey,
            sign_only,
            dump_transaction_message,
            blockhash_query,
            ref nonce_account,
            nonce_authority,
            memo,
            fee_payer,
            compute_unit_price,
        } => process_withdraw_from_vote_account(
            &rpc_client,
            config,
            vote_account_pubkey,
            *withdraw_authority,
            *withdraw_amount,
            destination_account_pubkey,
            *sign_only,
            *dump_transaction_message,
            blockhash_query,
            nonce_account.as_ref(),
            *nonce_authority,
            memo.as_ref(),
            *fee_payer,
            compute_unit_price.as_ref(),
        ),
        CliCommand::CloseVoteAccount {
            vote_account_pubkey,
            withdraw_authority,
            destination_account_pubkey,
            memo,
            fee_payer,
            compute_unit_price,
        } => process_close_vote_account(
            &rpc_client,
            config,
            vote_account_pubkey,
            *withdraw_authority,
            destination_account_pubkey,
            memo.as_ref(),
            *fee_payer,
            compute_unit_price.as_ref(),
        ),
        CliCommand::VoteAuthorize {
            vote_account_pubkey,
            new_authorized_pubkey,
            vote_authorize,
            sign_only,
            dump_transaction_message,
            blockhash_query,
            nonce_account,
            nonce_authority,
            memo,
            fee_payer,
            authorized,
            new_authorized,
            compute_unit_price,
        } => process_vote_authorize(
            &rpc_client,
            config,
            vote_account_pubkey,
            new_authorized_pubkey,
            *vote_authorize,
            *authorized,
            *new_authorized,
            *sign_only,
            *dump_transaction_message,
            blockhash_query,
            *nonce_account,
            *nonce_authority,
            memo.as_ref(),
            *fee_payer,
            compute_unit_price.as_ref(),
        ),
        CliCommand::VoteUpdateValidator {
            vote_account_pubkey,
            new_identity_account,
            withdraw_authority,
            sign_only,
            dump_transaction_message,
            blockhash_query,
            nonce_account,
            nonce_authority,
            memo,
            fee_payer,
            compute_unit_price,
        } => process_vote_update_validator(
            &rpc_client,
            config,
            vote_account_pubkey,
            *new_identity_account,
            *withdraw_authority,
            *sign_only,
            *dump_transaction_message,
            blockhash_query,
            *nonce_account,
            *nonce_authority,
            memo.as_ref(),
            *fee_payer,
            compute_unit_price.as_ref(),
        ),
        CliCommand::VoteUpdateCommission {
            vote_account_pubkey,
            commission,
            withdraw_authority,
            sign_only,
            dump_transaction_message,
            blockhash_query,
            nonce_account,
            nonce_authority,
            memo,
            fee_payer,
            compute_unit_price,
        } => process_vote_update_commission(
            &rpc_client,
            config,
            vote_account_pubkey,
            *commission,
            *withdraw_authority,
            *sign_only,
            *dump_transaction_message,
            blockhash_query,
            *nonce_account,
            *nonce_authority,
            memo.as_ref(),
            *fee_payer,
            compute_unit_price.as_ref(),
        ),

        // Wallet Commands

        // Request an airdrop from Solana Faucet;
        CliCommand::Airdrop { pubkey, lamports } => {
            process_airdrop(&rpc_client, config, pubkey, *lamports)
        }
        // Check client balance
        CliCommand::Balance {
            pubkey,
            use_lamports_unit,
        } => process_balance(&rpc_client, config, pubkey, *use_lamports_unit),
        // Confirm the last client transaction by signature
        CliCommand::Confirm(signature) => process_confirm(&rpc_client, config, signature),
        CliCommand::DecodeTransaction(transaction) => {
            process_decode_transaction(config, transaction)
        }
        CliCommand::ResolveSigner(path) => {
            if let Some(path) = path {
                Ok(path.to_string())
            } else {
                Ok("Signer is valid".to_string())
            }
        }
        CliCommand::ShowAccount {
            pubkey,
            output_file,
            use_lamports_unit,
        } => process_show_account(&rpc_client, config, pubkey, output_file, *use_lamports_unit),
        CliCommand::Transfer {
            amount,
            to,
            from,
            sign_only,
            dump_transaction_message,
            allow_unfunded_recipient,
            no_wait,
            ref blockhash_query,
            ref nonce_account,
            nonce_authority,
            memo,
            fee_payer,
            derived_address_seed,
            ref derived_address_program_id,
            compute_unit_price,
        } => process_transfer(
            &rpc_client,
            config,
            *amount,
            to,
            *from,
            *sign_only,
            *dump_transaction_message,
            *allow_unfunded_recipient,
            *no_wait,
            blockhash_query,
            nonce_account.as_ref(),
            *nonce_authority,
            memo.as_ref(),
            *fee_payer,
            derived_address_seed.clone(),
            derived_address_program_id.as_ref(),
            compute_unit_price.as_ref(),
        ),
        // Address Lookup Table Commands
        CliCommand::AddressLookupTable(subcommand) => {
            process_address_lookup_table_subcommand(rpc_client, config, subcommand)
        }
        CliCommand::SignOffchainMessage { message } => {
            process_sign_offchain_message(config, message)
        }
        CliCommand::VerifyOffchainSignature {
            signer_pubkey,
            signature,
            message,
        } => process_verify_offchain_signature(config, signer_pubkey, signature, message),
    }
}

pub fn request_and_confirm_airdrop(
    rpc_client: &RpcClient,
    config: &CliConfig,
    to_pubkey: &Pubkey,
    lamports: u64,
) -> ClientResult<Signature> {
    let recent_blockhash = rpc_client.get_latest_blockhash()?;
    let signature =
        rpc_client.request_airdrop_with_blockhash(to_pubkey, lamports, &recent_blockhash)?;
    rpc_client.confirm_transaction_with_spinner(
        &signature,
        &recent_blockhash,
        config.commitment,
    )?;
    Ok(signature)
}

fn common_error_adapter<E>(ix_error: &InstructionError) -> Option<E>
where
    E: 'static + std::error::Error + DecodeError<E> + FromPrimitive,
{
    if let InstructionError::Custom(code) = ix_error {
        E::decode_custom_error_to_enum(*code)
    } else {
        None
    }
}

pub fn log_instruction_custom_error<E>(
    result: ClientResult<Signature>,
    config: &CliConfig,
) -> ProcessResult
where
    E: 'static + std::error::Error + DecodeError<E> + FromPrimitive,
{
    log_instruction_custom_error_ex::<E, _>(result, config, common_error_adapter)
}

pub fn log_instruction_custom_error_ex<E, F>(
    result: ClientResult<Signature>,
    config: &CliConfig,
    error_adapter: F,
) -> ProcessResult
where
    E: 'static + std::error::Error + DecodeError<E> + FromPrimitive,
    F: Fn(&InstructionError) -> Option<E>,
{
    match result {
        Err(err) => {
            let maybe_tx_err = err.get_transaction_error();
            if let Some(TransactionError::InstructionError(_, ix_error)) = maybe_tx_err {
                if let Some(specific_error) = error_adapter(&ix_error) {
                    return Err(specific_error.into());
                }
            }
            Err(err.into())
        }
        Ok(sig) => {
            let signature = CliSignature {
                signature: sig.clone().to_string(),
            };
            Ok(config.output_format.formatted_string(&signature))
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        serde_json::json,
        solana_rpc_client::mock_sender_for_cli::SIGNATURE,
        solana_rpc_client_api::{
            request::RpcRequest,
            response::{Response, RpcResponseContext},
        },
        solana_rpc_client_nonce_utils::blockhash_query,
        solana_sdk::{
            pubkey::Pubkey,
            signature::{
                keypair_from_seed, read_keypair_file, write_keypair_file, Keypair, Presigner,
            },
            stake, system_program,
            transaction::TransactionError,
        },
        solana_transaction_status::TransactionConfirmationStatus,
    };

    fn make_tmp_path(name: &str) -> String {
        let out_dir = std::env::var("FARF_DIR").unwrap_or_else(|_| "farf".to_string());
        let keypair = Keypair::new();

        let path = format!("{}/tmp/{}-{}", out_dir, name, keypair.pubkey());

        // whack any possible collision
        let _ignored = std::fs::remove_dir_all(&path);
        // whack any possible collision
        let _ignored = std::fs::remove_file(&path);

        path
    }

    #[test]
    fn test_generate_unique_signers() {
        let matches = ArgMatches::default();

        let default_keypair = Keypair::new();
        let default_keypair_file = make_tmp_path("keypair_file");
        write_keypair_file(&default_keypair, &default_keypair_file).unwrap();

        let default_signer = DefaultSigner::new("keypair", &default_keypair_file);

        let signer_info = default_signer
            .generate_unique_signers(vec![], &matches, &mut None)
            .unwrap();
        assert_eq!(signer_info.signers.len(), 0);

        let signer_info = default_signer
            .generate_unique_signers(vec![None, None], &matches, &mut None)
            .unwrap();
        assert_eq!(signer_info.signers.len(), 1);
        assert_eq!(signer_info.index_of(None), Some(0));
        assert_eq!(
            signer_info.index_of(Some(solana_sdk::pubkey::new_rand())),
            None
        );

        let keypair0 = keypair_from_seed(&[1u8; 32]).unwrap();
        let keypair0_pubkey = keypair0.pubkey();
        let keypair0_clone = keypair_from_seed(&[1u8; 32]).unwrap();
        let keypair0_clone_pubkey = keypair0.pubkey();
        let signers = vec![None, Some(keypair0.into()), Some(keypair0_clone.into())];
        let signer_info = default_signer
            .generate_unique_signers(signers, &matches, &mut None)
            .unwrap();
        assert_eq!(signer_info.signers.len(), 2);
        assert_eq!(signer_info.index_of(None), Some(0));
        assert_eq!(signer_info.index_of(Some(keypair0_pubkey)), Some(1));
        assert_eq!(signer_info.index_of(Some(keypair0_clone_pubkey)), Some(1));

        let keypair0 = keypair_from_seed(&[1u8; 32]).unwrap();
        let keypair0_pubkey = keypair0.pubkey();
        let keypair0_clone = keypair_from_seed(&[1u8; 32]).unwrap();
        let signers = vec![Some(keypair0.into()), Some(keypair0_clone.into())];
        let signer_info = default_signer
            .generate_unique_signers(signers, &matches, &mut None)
            .unwrap();
        assert_eq!(signer_info.signers.len(), 1);
        assert_eq!(signer_info.index_of(Some(keypair0_pubkey)), Some(0));

        // Signers with the same pubkey are not distinct
        let keypair0 = keypair_from_seed(&[2u8; 32]).unwrap();
        let keypair0_pubkey = keypair0.pubkey();
        let keypair1 = keypair_from_seed(&[3u8; 32]).unwrap();
        let keypair1_pubkey = keypair1.pubkey();
        let message = vec![0, 1, 2, 3];
        let presigner0 = Presigner::new(&keypair0.pubkey(), &keypair0.sign_message(&message));
        let presigner0_pubkey = presigner0.pubkey();
        let presigner1 = Presigner::new(&keypair1.pubkey(), &keypair1.sign_message(&message));
        let presigner1_pubkey = presigner1.pubkey();
        let signers = vec![
            Some(keypair0.into()),
            Some(presigner0.into()),
            Some(presigner1.into()),
            Some(keypair1.into()),
        ];
        let signer_info = default_signer
            .generate_unique_signers(signers, &matches, &mut None)
            .unwrap();
        assert_eq!(signer_info.signers.len(), 2);
        assert_eq!(signer_info.index_of(Some(keypair0_pubkey)), Some(0));
        assert_eq!(signer_info.index_of(Some(keypair1_pubkey)), Some(1));
        assert_eq!(signer_info.index_of(Some(presigner0_pubkey)), Some(0));
        assert_eq!(signer_info.index_of(Some(presigner1_pubkey)), Some(1));
    }

    #[test]
    #[allow(clippy::cognitive_complexity)]
    fn test_cli_parse_command() {
        let test_commands = get_clap_app("test", "desc", "version");

        let pubkey = solana_sdk::pubkey::new_rand();
        let pubkey_string = format!("{pubkey}");

        let default_keypair = Keypair::new();
        let keypair_file = make_tmp_path("keypair_file");
        write_keypair_file(&default_keypair, &keypair_file).unwrap();
        let keypair = read_keypair_file(&keypair_file).unwrap();
        let default_signer = DefaultSigner::new("", &keypair_file);
        // Test Airdrop Subcommand
        let test_airdrop =
            test_commands
                .clone()
                .get_matches_from(vec!["test", "airdrop", "50", &pubkey_string]);
        assert_eq!(
            parse_command(&test_airdrop, &default_signer, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::Airdrop {
                    pubkey: Some(pubkey),
                    lamports: 50_000_000_000,
                },
                signers: vec![],
            }
        );

        // Test Balance Subcommand, incl pubkey and keypair-file inputs
        let test_balance = test_commands.clone().get_matches_from(vec![
            "test",
            "balance",
            &keypair.pubkey().to_string(),
        ]);
        assert_eq!(
            parse_command(&test_balance, &default_signer, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::Balance {
                    pubkey: Some(keypair.pubkey()),
                    use_lamports_unit: false,
                },
                signers: vec![],
            }
        );
        let test_balance = test_commands.clone().get_matches_from(vec![
            "test",
            "balance",
            &keypair_file,
            "--lamports",
        ]);
        assert_eq!(
            parse_command(&test_balance, &default_signer, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::Balance {
                    pubkey: Some(keypair.pubkey()),
                    use_lamports_unit: true,
                },
                signers: vec![],
            }
        );
        let test_balance =
            test_commands
                .clone()
                .get_matches_from(vec!["test", "balance", "--lamports"]);
        assert_eq!(
            parse_command(&test_balance, &default_signer, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::Balance {
                    pubkey: None,
                    use_lamports_unit: true,
                },
                signers: vec![read_keypair_file(&keypair_file).unwrap().into()],
            }
        );

        // Test Confirm Subcommand
        let signature = Signature::new(&[1; 64]);
        let signature_string = format!("{signature:?}");
        let test_confirm =
            test_commands
                .clone()
                .get_matches_from(vec!["test", "confirm", &signature_string]);
        assert_eq!(
            parse_command(&test_confirm, &default_signer, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::Confirm(signature),
                signers: vec![],
            }
        );
        let test_bad_signature = test_commands
            .clone()
            .get_matches_from(vec!["test", "confirm", "deadbeef"]);
        assert!(parse_command(&test_bad_signature, &default_signer, &mut None).is_err());

        // Test CreateAddressWithSeed
        let from_pubkey = Some(solana_sdk::pubkey::new_rand());
        let from_str = from_pubkey.unwrap().to_string();
        for (name, program_id) in &[
            ("STAKE", stake::program::id()),
            ("VOTE", solana_vote_program::id()),
            ("NONCE", system_program::id()),
        ] {
            let test_create_address_with_seed = test_commands.clone().get_matches_from(vec![
                "test",
                "create-address-with-seed",
                "seed",
                name,
                "--from",
                &from_str,
            ]);
            assert_eq!(
                parse_command(&test_create_address_with_seed, &default_signer, &mut None).unwrap(),
                CliCommandInfo {
                    command: CliCommand::CreateAddressWithSeed {
                        from_pubkey,
                        seed: "seed".to_string(),
                        program_id: *program_id
                    },
                    signers: vec![],
                }
            );
        }
        let test_create_address_with_seed = test_commands.clone().get_matches_from(vec![
            "test",
            "create-address-with-seed",
            "seed",
            "STAKE",
        ]);
        assert_eq!(
            parse_command(&test_create_address_with_seed, &default_signer, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::CreateAddressWithSeed {
                    from_pubkey: None,
                    seed: "seed".to_string(),
                    program_id: stake::program::id(),
                },
                signers: vec![read_keypair_file(&keypair_file).unwrap().into()],
            }
        );

        // Test ResolveSigner Subcommand, SignerSource::Filepath
        let test_resolve_signer =
            test_commands
                .clone()
                .get_matches_from(vec!["test", "resolve-signer", &keypair_file]);
        assert_eq!(
            parse_command(&test_resolve_signer, &default_signer, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::ResolveSigner(Some(keypair_file.clone())),
                signers: vec![],
            }
        );
        // Test ResolveSigner Subcommand, SignerSource::Pubkey (Presigner)
        let test_resolve_signer =
            test_commands
                .clone()
                .get_matches_from(vec!["test", "resolve-signer", &pubkey_string]);
        assert_eq!(
            parse_command(&test_resolve_signer, &default_signer, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::ResolveSigner(Some(pubkey.to_string())),
                signers: vec![],
            }
        );

        // Test SignOffchainMessage
        let test_sign_offchain = test_commands.clone().get_matches_from(vec![
            "test",
            "sign-offchain-message",
            "Test Message",
        ]);
        let message = OffchainMessage::new(0, b"Test Message").unwrap();
        assert_eq!(
            parse_command(&test_sign_offchain, &default_signer, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::SignOffchainMessage {
                    message: message.clone()
                },
                signers: vec![read_keypair_file(&keypair_file).unwrap().into()],
            }
        );

        // Test VerifyOffchainSignature
        let signature = keypair.sign_message(&message.serialize().unwrap());
        let test_verify_offchain = test_commands.clone().get_matches_from(vec![
            "test",
            "verify-offchain-signature",
            "Test Message",
            &signature.to_string(),
        ]);
        assert_eq!(
            parse_command(&test_verify_offchain, &default_signer, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::VerifyOffchainSignature {
                    signer_pubkey: None,
                    signature,
                    message
                },
                signers: vec![read_keypair_file(&keypair_file).unwrap().into()],
            }
        );
    }

    #[test]
    #[allow(clippy::cognitive_complexity)]
    fn test_cli_process_command() {
        // Success cases
        let mut config = CliConfig {
            rpc_client: Some(Arc::new(RpcClient::new_mock("succeeds".to_string()))),
            json_rpc_url: "http://127.0.0.1:8899".to_string(),
            ..CliConfig::default()
        };

        let keypair = Keypair::new();
        let pubkey = keypair.pubkey().to_string();
        config.signers = vec![&keypair];
        config.command = CliCommand::Address;
        assert_eq!(process_command(&config).unwrap(), pubkey);

        config.command = CliCommand::Balance {
            pubkey: None,
            use_lamports_unit: true,
        };
        assert_eq!(process_command(&config).unwrap(), "50 lamports");

        config.command = CliCommand::Balance {
            pubkey: None,
            use_lamports_unit: false,
        };
        assert_eq!(process_command(&config).unwrap(), "0.00000005 SOL");

        let good_signature = Signature::new(&bs58::decode(SIGNATURE).into_vec().unwrap());
        config.command = CliCommand::Confirm(good_signature);
        assert_eq!(
            process_command(&config).unwrap(),
            format!("{:?}", TransactionConfirmationStatus::Finalized)
        );

        let bob_keypair = Keypair::new();
        let bob_pubkey = bob_keypair.pubkey();
        let identity_keypair = Keypair::new();
        config.command = CliCommand::CreateVoteAccount {
            vote_account: 1,
            seed: None,
            identity_account: 2,
            authorized_voter: Some(bob_pubkey),
            authorized_withdrawer: bob_pubkey,
            commission: 0,
            sign_only: false,
            dump_transaction_message: false,
            blockhash_query: BlockhashQuery::All(blockhash_query::Source::Cluster),
            nonce_account: None,
            nonce_authority: 0,
            memo: None,
            fee_payer: 0,
            compute_unit_price: None,
        };
        config.signers = vec![&keypair, &bob_keypair, &identity_keypair];
        let result = process_command(&config);
        assert!(result.is_ok());

        let vote_account_info_response = json!(Response {
            context: RpcResponseContext {
                slot: 1,
                api_version: None
            },
            value: json!({
                "data": ["KLUv/QBYNQIAtAIBAAAAbnoc3Smwt4/ROvTFWY/v9O8qlxZuPKby5Pv8zYBQW/EFAAEAAB8ACQD6gx92zAiAAecDP4B2XeEBSIx7MQeung==", "base64+zstd"],
                "lamports": 42,
                "owner": "Vote111111111111111111111111111111111111111",
                "executable": false,
                "rentEpoch": 1,
            }),
        });
        let mut mocks = HashMap::new();
        mocks.insert(RpcRequest::GetAccountInfo, vote_account_info_response);
        let rpc_client = RpcClient::new_mock_with_mocks("".to_string(), mocks);
        let mut vote_config = CliConfig {
            rpc_client: Some(Arc::new(rpc_client)),
            json_rpc_url: "http://127.0.0.1:8899".to_string(),
            ..CliConfig::default()
        };
        let current_authority = keypair_from_seed(&[5; 32]).unwrap();
        let new_authorized_pubkey = solana_sdk::pubkey::new_rand();
        vote_config.signers = vec![&current_authority];
        vote_config.command = CliCommand::VoteAuthorize {
            vote_account_pubkey: bob_pubkey,
            new_authorized_pubkey,
            vote_authorize: VoteAuthorize::Withdrawer,
            sign_only: false,
            dump_transaction_message: false,
            blockhash_query: BlockhashQuery::All(blockhash_query::Source::Cluster),
            nonce_account: None,
            nonce_authority: 0,
            memo: None,
            fee_payer: 0,
            authorized: 0,
            new_authorized: None,
            compute_unit_price: None,
        };
        let result = process_command(&vote_config);
        assert!(result.is_ok());

        let new_identity_keypair = Keypair::new();
        config.signers = vec![&keypair, &bob_keypair, &new_identity_keypair];
        config.command = CliCommand::VoteUpdateValidator {
            vote_account_pubkey: bob_pubkey,
            new_identity_account: 2,
            withdraw_authority: 1,
            sign_only: false,
            dump_transaction_message: false,
            blockhash_query: BlockhashQuery::All(blockhash_query::Source::Cluster),
            nonce_account: None,
            nonce_authority: 0,
            memo: None,
            fee_payer: 0,
            compute_unit_price: None,
        };
        let result = process_command(&config);
        assert!(result.is_ok());

        let bob_keypair = Keypair::new();
        let bob_pubkey = bob_keypair.pubkey();
        let custodian = solana_sdk::pubkey::new_rand();
        config.command = CliCommand::CreateStakeAccount {
            stake_account: 1,
            seed: None,
            staker: None,
            withdrawer: None,
            withdrawer_signer: None,
            lockup: Lockup {
                epoch: 0,
                unix_timestamp: 0,
                custodian,
            },
            amount: SpendAmount::Some(30),
            sign_only: false,
            dump_transaction_message: false,
            blockhash_query: BlockhashQuery::All(blockhash_query::Source::Cluster),
            nonce_account: None,
            nonce_authority: 0,
            memo: None,
            fee_payer: 0,
            from: 0,
            compute_unit_price: None,
        };
        config.signers = vec![&keypair, &bob_keypair];
        let result = process_command(&config);
        assert!(result.is_ok());

        let stake_account_pubkey = solana_sdk::pubkey::new_rand();
        let to_pubkey = solana_sdk::pubkey::new_rand();
        config.command = CliCommand::WithdrawStake {
            stake_account_pubkey,
            destination_account_pubkey: to_pubkey,
            amount: SpendAmount::All,
            withdraw_authority: 0,
            custodian: None,
            sign_only: false,
            dump_transaction_message: false,
            blockhash_query: BlockhashQuery::All(blockhash_query::Source::Cluster),
            nonce_account: None,
            nonce_authority: 0,
            memo: None,
            seed: None,
            fee_payer: 0,
            compute_unit_price: None,
        };
        config.signers = vec![&keypair];
        let result = process_command(&config);
        assert!(result.is_ok());

        let stake_account_pubkey = solana_sdk::pubkey::new_rand();
        config.command = CliCommand::DeactivateStake {
            stake_account_pubkey,
            stake_authority: 0,
            sign_only: false,
            dump_transaction_message: false,
            deactivate_delinquent: false,
            blockhash_query: BlockhashQuery::default(),
            nonce_account: None,
            nonce_authority: 0,
            memo: None,
            seed: None,
            fee_payer: 0,
            compute_unit_price: None,
        };
        let result = process_command(&config);
        assert!(result.is_ok());

        let stake_account_pubkey = solana_sdk::pubkey::new_rand();
        let split_stake_account = Keypair::new();
        config.command = CliCommand::SplitStake {
            stake_account_pubkey,
            stake_authority: 0,
            sign_only: false,
            dump_transaction_message: false,
            blockhash_query: BlockhashQuery::default(),
            nonce_account: None,
            nonce_authority: 0,
            memo: None,
            split_stake_account: 1,
            seed: None,
            lamports: 30,
            fee_payer: 0,
            compute_unit_price: None,
        };
        config.signers = vec![&keypair, &split_stake_account];
        let result = process_command(&config);
        assert!(result.is_ok());

        let stake_account_pubkey = solana_sdk::pubkey::new_rand();
        let source_stake_account_pubkey = solana_sdk::pubkey::new_rand();
        let merge_stake_account = Keypair::new();
        config.command = CliCommand::MergeStake {
            stake_account_pubkey,
            source_stake_account_pubkey,
            stake_authority: 1,
            sign_only: false,
            dump_transaction_message: false,
            blockhash_query: BlockhashQuery::default(),
            nonce_account: None,
            nonce_authority: 0,
            memo: None,
            fee_payer: 0,
            compute_unit_price: None,
        };
        config.signers = vec![&keypair, &merge_stake_account];
        let result = process_command(&config);
        assert!(result.is_ok());

        config.command = CliCommand::GetSlot;
        assert_eq!(process_command(&config).unwrap(), "0");

        config.command = CliCommand::GetTransactionCount;
        assert_eq!(process_command(&config).unwrap(), "1234");

        // CreateAddressWithSeed
        let from_pubkey = solana_sdk::pubkey::new_rand();
        config.signers = vec![];
        config.command = CliCommand::CreateAddressWithSeed {
            from_pubkey: Some(from_pubkey),
            seed: "seed".to_string(),
            program_id: stake::program::id(),
        };
        let address = process_command(&config);
        let expected_address =
            Pubkey::create_with_seed(&from_pubkey, "seed", &stake::program::id()).unwrap();
        assert_eq!(address.unwrap(), expected_address.to_string());

        // Need airdrop cases
        let to = solana_sdk::pubkey::new_rand();
        config.signers = vec![&keypair];
        config.command = CliCommand::Airdrop {
            pubkey: Some(to),
            lamports: 50,
        };
        assert!(process_command(&config).is_ok());

        // sig_not_found case
        config.rpc_client = Some(Arc::new(RpcClient::new_mock("sig_not_found".to_string())));
        let missing_signature = Signature::new(&bs58::decode("5VERv8NMvzbJMEkV8xnrLkEaWRtSz9CosKDYjCJjBRnbJLgp8uirBgmQpjKhoR4tjF3ZpRzrFmBV6UjKdiSZkQUW").into_vec().unwrap());
        config.command = CliCommand::Confirm(missing_signature);
        assert_eq!(process_command(&config).unwrap(), "Not found");

        // Tx error case
        config.rpc_client = Some(Arc::new(RpcClient::new_mock("account_in_use".to_string())));
        let any_signature = Signature::new(&bs58::decode(SIGNATURE).into_vec().unwrap());
        config.command = CliCommand::Confirm(any_signature);
        assert_eq!(
            process_command(&config).unwrap(),
            format!("Transaction failed: {}", TransactionError::AccountInUse)
        );

        // Failure cases
        config.rpc_client = Some(Arc::new(RpcClient::new_mock("fails".to_string())));

        config.command = CliCommand::Airdrop {
            pubkey: None,
            lamports: 50,
        };
        assert!(process_command(&config).is_err());

        config.command = CliCommand::Balance {
            pubkey: None,
            use_lamports_unit: false,
        };
        assert!(process_command(&config).is_err());

        let bob_keypair = Keypair::new();
        let identity_keypair = Keypair::new();
        config.command = CliCommand::CreateVoteAccount {
            vote_account: 1,
            seed: None,
            identity_account: 2,
            authorized_voter: Some(bob_pubkey),
            authorized_withdrawer: bob_pubkey,
            commission: 0,
            sign_only: false,
            dump_transaction_message: false,
            blockhash_query: BlockhashQuery::All(blockhash_query::Source::Cluster),
            nonce_account: None,
            nonce_authority: 0,
            memo: None,
            fee_payer: 0,
            compute_unit_price: None,
        };
        config.signers = vec![&keypair, &bob_keypair, &identity_keypair];
        assert!(process_command(&config).is_err());

        config.command = CliCommand::VoteAuthorize {
            vote_account_pubkey: bob_pubkey,
            new_authorized_pubkey: bob_pubkey,
            vote_authorize: VoteAuthorize::Voter,
            sign_only: false,
            dump_transaction_message: false,
            blockhash_query: BlockhashQuery::All(blockhash_query::Source::Cluster),
            nonce_account: None,
            nonce_authority: 0,
            memo: None,
            fee_payer: 0,
            authorized: 0,
            new_authorized: None,
            compute_unit_price: None,
        };
        assert!(process_command(&config).is_err());

        config.command = CliCommand::VoteUpdateValidator {
            vote_account_pubkey: bob_pubkey,
            new_identity_account: 1,
            withdraw_authority: 1,
            sign_only: false,
            dump_transaction_message: false,
            blockhash_query: BlockhashQuery::All(blockhash_query::Source::Cluster),
            nonce_account: None,
            nonce_authority: 0,
            memo: None,
            fee_payer: 0,
            compute_unit_price: None,
        };
        assert!(process_command(&config).is_err());

        config.command = CliCommand::GetSlot;
        assert!(process_command(&config).is_err());

        config.command = CliCommand::GetTransactionCount;
        assert!(process_command(&config).is_err());

        let message = OffchainMessage::new(0, b"Test Message").unwrap();
        config.command = CliCommand::SignOffchainMessage {
            message: message.clone(),
        };
        config.signers = vec![&keypair];
        let result = process_command(&config);
        assert!(result.is_ok());

        config.command = CliCommand::VerifyOffchainSignature {
            signer_pubkey: None,
            signature: result.unwrap().parse().unwrap(),
            message,
        };
        config.signers = vec![&keypair];
        let result = process_command(&config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_transfer_subcommand() {
        let test_commands = get_clap_app("test", "desc", "version");

        let default_keypair = Keypair::new();
        let default_keypair_file = make_tmp_path("keypair_file");
        write_keypair_file(&default_keypair, &default_keypair_file).unwrap();
        let default_signer = DefaultSigner::new("", &default_keypair_file);

        //Test Transfer Subcommand, SOL
        let from_keypair = keypair_from_seed(&[0u8; 32]).unwrap();
        let from_pubkey = from_keypair.pubkey();
        let from_string = from_pubkey.to_string();
        let to_keypair = keypair_from_seed(&[1u8; 32]).unwrap();
        let to_pubkey = to_keypair.pubkey();
        let to_string = to_pubkey.to_string();
        let test_transfer = test_commands
            .clone()
            .get_matches_from(vec!["test", "transfer", &to_string, "42"]);
        assert_eq!(
            parse_command(&test_transfer, &default_signer, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::Transfer {
                    amount: SpendAmount::Some(42_000_000_000),
                    to: to_pubkey,
                    from: 0,
                    sign_only: false,
                    dump_transaction_message: false,
                    allow_unfunded_recipient: false,
                    no_wait: false,
                    blockhash_query: BlockhashQuery::All(blockhash_query::Source::Cluster),
                    nonce_account: None,
                    nonce_authority: 0,
                    memo: None,
                    fee_payer: 0,
                    derived_address_seed: None,
                    derived_address_program_id: None,
                    compute_unit_price: None,
                },
                signers: vec![read_keypair_file(&default_keypair_file).unwrap().into()],
            }
        );

        // Test Transfer ALL
        let test_transfer = test_commands
            .clone()
            .get_matches_from(vec!["test", "transfer", &to_string, "ALL"]);
        assert_eq!(
            parse_command(&test_transfer, &default_signer, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::Transfer {
                    amount: SpendAmount::All,
                    to: to_pubkey,
                    from: 0,
                    sign_only: false,
                    dump_transaction_message: false,
                    allow_unfunded_recipient: false,
                    no_wait: false,
                    blockhash_query: BlockhashQuery::All(blockhash_query::Source::Cluster),
                    nonce_account: None,
                    nonce_authority: 0,
                    memo: None,
                    fee_payer: 0,
                    derived_address_seed: None,
                    derived_address_program_id: None,
                    compute_unit_price: None,
                },
                signers: vec![read_keypair_file(&default_keypair_file).unwrap().into()],
            }
        );

        // Test Transfer no-wait and --allow-unfunded-recipient
        let test_transfer = test_commands.clone().get_matches_from(vec![
            "test",
            "transfer",
            "--no-wait",
            "--allow-unfunded-recipient",
            &to_string,
            "42",
        ]);
        assert_eq!(
            parse_command(&test_transfer, &default_signer, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::Transfer {
                    amount: SpendAmount::Some(42_000_000_000),
                    to: to_pubkey,
                    from: 0,
                    sign_only: false,
                    dump_transaction_message: false,
                    allow_unfunded_recipient: true,
                    no_wait: true,
                    blockhash_query: BlockhashQuery::All(blockhash_query::Source::Cluster),
                    nonce_account: None,
                    nonce_authority: 0,
                    memo: None,
                    fee_payer: 0,
                    derived_address_seed: None,
                    derived_address_program_id: None,
                    compute_unit_price: None,
                },
                signers: vec![read_keypair_file(&default_keypair_file).unwrap().into()],
            }
        );

        //Test Transfer Subcommand, offline sign
        let blockhash = Hash::new(&[1u8; 32]);
        let blockhash_string = blockhash.to_string();
        let test_transfer = test_commands.clone().get_matches_from(vec![
            "test",
            "transfer",
            &to_string,
            "42",
            "--blockhash",
            &blockhash_string,
            "--sign-only",
        ]);
        assert_eq!(
            parse_command(&test_transfer, &default_signer, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::Transfer {
                    amount: SpendAmount::Some(42_000_000_000),
                    to: to_pubkey,
                    from: 0,
                    sign_only: true,
                    dump_transaction_message: false,
                    allow_unfunded_recipient: false,
                    no_wait: false,
                    blockhash_query: BlockhashQuery::None(blockhash),
                    nonce_account: None,
                    nonce_authority: 0,
                    memo: None,
                    fee_payer: 0,
                    derived_address_seed: None,
                    derived_address_program_id: None,
                    compute_unit_price: None,
                },
                signers: vec![read_keypair_file(&default_keypair_file).unwrap().into()],
            }
        );

        //Test Transfer Subcommand, submit offline `from`
        let from_sig = from_keypair.sign_message(&[0u8]);
        let from_signer = format!("{from_pubkey}={from_sig}");
        let test_transfer = test_commands.clone().get_matches_from(vec![
            "test",
            "transfer",
            &to_string,
            "42",
            "--from",
            &from_string,
            "--fee-payer",
            &from_string,
            "--signer",
            &from_signer,
            "--blockhash",
            &blockhash_string,
        ]);
        assert_eq!(
            parse_command(&test_transfer, &default_signer, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::Transfer {
                    amount: SpendAmount::Some(42_000_000_000),
                    to: to_pubkey,
                    from: 0,
                    sign_only: false,
                    dump_transaction_message: false,
                    allow_unfunded_recipient: false,
                    no_wait: false,
                    blockhash_query: BlockhashQuery::FeeCalculator(
                        blockhash_query::Source::Cluster,
                        blockhash
                    ),
                    nonce_account: None,
                    nonce_authority: 0,
                    memo: None,
                    fee_payer: 0,
                    derived_address_seed: None,
                    derived_address_program_id: None,
                    compute_unit_price: None,
                },
                signers: vec![Presigner::new(&from_pubkey, &from_sig).into()],
            }
        );

        //Test Transfer Subcommand, with nonce
        let nonce_address = Pubkey::new(&[1u8; 32]);
        let nonce_address_string = nonce_address.to_string();
        let nonce_authority = keypair_from_seed(&[2u8; 32]).unwrap();
        let nonce_authority_file = make_tmp_path("nonce_authority_file");
        write_keypair_file(&nonce_authority, &nonce_authority_file).unwrap();
        let test_transfer = test_commands.clone().get_matches_from(vec![
            "test",
            "transfer",
            &to_string,
            "42",
            "--blockhash",
            &blockhash_string,
            "--nonce",
            &nonce_address_string,
            "--nonce-authority",
            &nonce_authority_file,
        ]);
        assert_eq!(
            parse_command(&test_transfer, &default_signer, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::Transfer {
                    amount: SpendAmount::Some(42_000_000_000),
                    to: to_pubkey,
                    from: 0,
                    sign_only: false,
                    dump_transaction_message: false,
                    allow_unfunded_recipient: false,
                    no_wait: false,
                    blockhash_query: BlockhashQuery::FeeCalculator(
                        blockhash_query::Source::NonceAccount(nonce_address),
                        blockhash
                    ),
                    nonce_account: Some(nonce_address),
                    nonce_authority: 1,
                    memo: None,
                    fee_payer: 0,
                    derived_address_seed: None,
                    derived_address_program_id: None,
                    compute_unit_price: None,
                },
                signers: vec![
                    read_keypair_file(&default_keypair_file).unwrap().into(),
                    read_keypair_file(&nonce_authority_file).unwrap().into()
                ],
            }
        );

        //Test Transfer Subcommand, with seed
        let derived_address_seed = "seed".to_string();
        let derived_address_program_id = "STAKE";
        let test_transfer = test_commands.clone().get_matches_from(vec![
            "test",
            "transfer",
            &to_string,
            "42",
            "--derived-address-seed",
            &derived_address_seed,
            "--derived-address-program-id",
            derived_address_program_id,
        ]);
        assert_eq!(
            parse_command(&test_transfer, &default_signer, &mut None).unwrap(),
            CliCommandInfo {
                command: CliCommand::Transfer {
                    amount: SpendAmount::Some(42_000_000_000),
                    to: to_pubkey,
                    from: 0,
                    sign_only: false,
                    dump_transaction_message: false,
                    allow_unfunded_recipient: false,
                    no_wait: false,
                    blockhash_query: BlockhashQuery::All(blockhash_query::Source::Cluster),
                    nonce_account: None,
                    nonce_authority: 0,
                    memo: None,
                    fee_payer: 0,
                    derived_address_seed: Some(derived_address_seed),
                    derived_address_program_id: Some(stake::program::id()),
                    compute_unit_price: None,
                },
                signers: vec![read_keypair_file(&default_keypair_file).unwrap().into(),],
            }
        );
    }
}
