use {
    solana_accounts_db::inline_spl_token,
    solana_sdk::{
        account::{Account, AccountSharedData},
        feature::{self, Feature},
        feature_set::FeatureSet,
        fee_calculator::FeeRateGovernor,
        genesis_config::{ClusterType, GenesisConfig},
        native_token::sol_to_lamports,
        pubkey::Pubkey,
        rent::Rent,
        signature::{Keypair, Signer},
        stake::state::StakeStateV2,
        system_program,
    },
    solana_stake_program::stake_state,
    solana_vote_program::vote_state,
    std::borrow::Borrow,
};

// Default amount received by the validator
const VALIDATOR_LAMPORTS: u64 = 42;

// fun fact: rustc is very close to make this const fn.
pub fn bootstrap_validator_stake_lamports() -> u64 {
    Rent::default().minimum_balance(StakeStateV2::size_of())
}

// Number of lamports automatically used for genesis accounts
pub const fn genesis_sysvar_and_builtin_program_lamports() -> u64 {
    const NUM_BUILTIN_PROGRAMS: u64 = 9;
    const NUM_PRECOMPILES: u64 = 2;
    const FEES_SYSVAR_MIN_BALANCE: u64 = 946_560;
    const STAKE_HISTORY_MIN_BALANCE: u64 = 114_979_200;
    const CLOCK_SYSVAR_MIN_BALANCE: u64 = 1_169_280;
    const RENT_SYSVAR_MIN_BALANCE: u64 = 1_009_200;
    const EPOCH_SCHEDULE_SYSVAR_MIN_BALANCE: u64 = 1_120_560;
    const RECENT_BLOCKHASHES_SYSVAR_MIN_BALANCE: u64 = 42_706_560;

    FEES_SYSVAR_MIN_BALANCE
        + STAKE_HISTORY_MIN_BALANCE
        + CLOCK_SYSVAR_MIN_BALANCE
        + RENT_SYSVAR_MIN_BALANCE
        + EPOCH_SCHEDULE_SYSVAR_MIN_BALANCE
        + RECENT_BLOCKHASHES_SYSVAR_MIN_BALANCE
        + NUM_BUILTIN_PROGRAMS
        + NUM_PRECOMPILES
}

pub struct ValidatorVoteKeypairs {
    pub node_keypair: Keypair,
    pub vote_keypair: Keypair,
    pub stake_keypair: Keypair,
}

impl ValidatorVoteKeypairs {
    pub fn new(node_keypair: Keypair, vote_keypair: Keypair, stake_keypair: Keypair) -> Self {
        Self {
            node_keypair,
            vote_keypair,
            stake_keypair,
        }
    }

    pub fn new_rand() -> Self {
        Self {
            node_keypair: Keypair::new(),
            vote_keypair: Keypair::new(),
            stake_keypair: Keypair::new(),
        }
    }
}

pub struct GenesisConfigInfo {
    pub genesis_config: GenesisConfig,
    pub mint_keypair: Keypair,
    pub voting_keypair: Keypair,
    pub validator_pubkey: Pubkey,
}

pub fn create_genesis_config(mint_lamports: u64) -> GenesisConfigInfo {
    // Note that zero lamports for validator stake will result in stake account
    // not being stored in accounts-db but still cached in bank stakes. This
    // causes discrepancy between cached stakes accounts in bank and
    // accounts-db which in particular will break snapshots test.
    create_genesis_config_with_leader(
        mint_lamports,
        &solana_sdk::pubkey::new_rand(), // validator_pubkey
        0,                               // validator_stake_lamports
    )
}

pub fn create_genesis_config_with_vote_accounts(
    mint_lamports: u64,
    voting_keypairs: &[impl Borrow<ValidatorVoteKeypairs>],
    stakes: Vec<u64>,
) -> GenesisConfigInfo {
    create_genesis_config_with_vote_accounts_and_cluster_type(
        mint_lamports,
        voting_keypairs,
        stakes,
        ClusterType::Development,
    )
}

pub fn create_genesis_config_with_vote_accounts_and_cluster_type(
    mint_lamports: u64,
    voting_keypairs: &[impl Borrow<ValidatorVoteKeypairs>],
    stakes: Vec<u64>,
    cluster_type: ClusterType,
) -> GenesisConfigInfo {
    assert!(!voting_keypairs.is_empty());
    assert_eq!(voting_keypairs.len(), stakes.len());

    let mint_keypair = Keypair::new();
    let voting_keypair = voting_keypairs[0].borrow().vote_keypair.insecure_clone();

    let validator_pubkey = voting_keypairs[0].borrow().node_keypair.pubkey();
    let genesis_config = create_genesis_config_with_leader_ex(
        mint_lamports,
        &mint_keypair.pubkey(),
        &validator_pubkey,
        &voting_keypairs[0].borrow().vote_keypair.pubkey(),
        &voting_keypairs[0].borrow().stake_keypair.pubkey(),
        stakes[0],
        VALIDATOR_LAMPORTS,
        FeeRateGovernor::new(0, 0), // most tests can't handle transaction fees
        Rent::free(),               // most tests don't expect rent
        cluster_type,
        vec![],
    );

    let mut genesis_config_info = GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        voting_keypair,
        validator_pubkey,
    };

    for (validator_voting_keypairs, stake) in voting_keypairs[1..].iter().zip(&stakes[1..]) {
        let node_pubkey = validator_voting_keypairs.borrow().node_keypair.pubkey();
        let vote_pubkey = validator_voting_keypairs.borrow().vote_keypair.pubkey();
        let stake_pubkey = validator_voting_keypairs.borrow().stake_keypair.pubkey();

        // Create accounts
        let node_account = Account::new(VALIDATOR_LAMPORTS, 0, &system_program::id());
        let vote_account = vote_state::create_account(&vote_pubkey, &node_pubkey, 0, *stake);
        let stake_account = Account::from(stake_state::create_account(
            &stake_pubkey,
            &vote_pubkey,
            &vote_account,
            &genesis_config_info.genesis_config.rent,
            *stake,
        ));

        let vote_account = Account::from(vote_account);

        // Put newly created accounts into genesis
        genesis_config_info.genesis_config.accounts.extend(vec![
            (node_pubkey, node_account),
            (vote_pubkey, vote_account),
            (stake_pubkey, stake_account),
        ]);
    }

    genesis_config_info
}

pub fn create_genesis_config_with_leader(
    mint_lamports: u64,
    validator_pubkey: &Pubkey,
    validator_stake_lamports: u64,
) -> GenesisConfigInfo {
    let mint_keypair = Keypair::new();
    let voting_keypair = Keypair::new();

    let genesis_config = create_genesis_config_with_leader_ex(
        mint_lamports,
        &mint_keypair.pubkey(),
        validator_pubkey,
        &voting_keypair.pubkey(),
        &solana_sdk::pubkey::new_rand(),
        validator_stake_lamports,
        VALIDATOR_LAMPORTS,
        FeeRateGovernor::new(0, 0), // most tests can't handle transaction fees
        Rent::free(),               // most tests don't expect rent
        ClusterType::Development,
        vec![],
    );

    GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        voting_keypair,
        validator_pubkey: *validator_pubkey,
    }
}

pub fn activate_all_features(genesis_config: &mut GenesisConfig) {
    // Activate all features at genesis in development mode
    for feature_id in FeatureSet::default().inactive {
        activate_feature(genesis_config, feature_id);
    }
}

pub fn activate_feature(genesis_config: &mut GenesisConfig, feature_id: Pubkey) {
    genesis_config.accounts.insert(
        feature_id,
        Account::from(feature::create_account(
            &Feature {
                activated_at: Some(0),
            },
            std::cmp::max(genesis_config.rent.minimum_balance(Feature::size_of()), 1),
        )),
    );
}

#[allow(clippy::too_many_arguments)]
pub fn create_genesis_config_with_leader_ex(
    mint_lamports: u64,
    mint_pubkey: &Pubkey,
    validator_pubkey: &Pubkey,
    validator_vote_account_pubkey: &Pubkey,
    validator_stake_account_pubkey: &Pubkey,
    validator_stake_lamports: u64,
    validator_lamports: u64,
    fee_rate_governor: FeeRateGovernor,
    rent: Rent,
    cluster_type: ClusterType,
    mut initial_accounts: Vec<(Pubkey, AccountSharedData)>,
) -> GenesisConfig {
    let validator_vote_account = vote_state::create_account(
        validator_vote_account_pubkey,
        validator_pubkey,
        0,
        validator_stake_lamports,
    );

    let validator_stake_account = stake_state::create_account(
        validator_stake_account_pubkey,
        validator_vote_account_pubkey,
        &validator_vote_account,
        &rent,
        validator_stake_lamports,
    );

    initial_accounts.push((
        *mint_pubkey,
        AccountSharedData::new(mint_lamports, 0, &system_program::id()),
    ));
    initial_accounts.push((
        *validator_pubkey,
        AccountSharedData::new(validator_lamports, 0, &system_program::id()),
    ));
    initial_accounts.push((*validator_vote_account_pubkey, validator_vote_account));
    initial_accounts.push((*validator_stake_account_pubkey, validator_stake_account));

    let native_mint_account = solana_sdk::account::AccountSharedData::from(Account {
        owner: inline_spl_token::id(),
        data: inline_spl_token::native_mint::ACCOUNT_DATA.to_vec(),
        lamports: sol_to_lamports(1.),
        executable: false,
        rent_epoch: 1,
    });
    initial_accounts.push((inline_spl_token::native_mint::id(), native_mint_account));

    let mut genesis_config = GenesisConfig {
        accounts: initial_accounts
            .iter()
            .cloned()
            .map(|(key, account)| (key, Account::from(account)))
            .collect(),
        fee_rate_governor,
        rent,
        cluster_type,
        ..GenesisConfig::default()
    };

    solana_stake_program::add_genesis_accounts(&mut genesis_config);
    if genesis_config.cluster_type == ClusterType::Development {
        activate_all_features(&mut genesis_config);
    }

    genesis_config
}
