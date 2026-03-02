Solana Logo
Command Line ToolsCommand ExamplesStaking
On this page
Staking SOL with the Solana CLI
After you have received SOL, you might consider putting it to use by delegating stake to a validator. Stake is what we call tokens in a stake account. Solana weights validator votes by the amount of stake delegated to them, which gives those validators more influence in determining then next valid block of transactions in the blockchain. Solana then generates new SOL periodically to reward stakers and validators. You earn more rewards the more stake you delegate.

INFO
For an overview of staking, read first the Staking and Inflation FAQ.

Create a Stake Account
To delegate stake, you will need to transfer some tokens into a stake account. To create an account, you will need a keypair. Its public key will be used as the stake account address. No need for a password or encryption here; this keypair will be discarded right after creating the stake account.

solana-keygen new --no-passphrase -o stake-account.json


The output will contain the public key after the text pubkey:.

pubkey: GKvqsuNcnwWqPzzuhLmGi4rzzh55FhJtGizkhHaEJqiV


Copy the public key and store it for safekeeping. You will need it any time you want to perform an action on the stake account you create next.

Now, create a stake account:

solana create-stake-account --from <KEYPAIR> stake-account.json <AMOUNT> \
    --stake-authority <KEYPAIR> --withdraw-authority <KEYPAIR> \
    --fee-payer <KEYPAIR>


<AMOUNT> tokens are transferred from the account at the "from" <KEYPAIR> to a new stake account at the public key of stake-account.json.

The stake-account.json file can now be discarded. To authorize additional actions, you will use the --stake-authority or --withdraw-authority keypair, not stake-account.json.

View the new stake account with the solana stake-account command:

solana stake-account <STAKE_ACCOUNT_ADDRESS>


The output will look similar to this:

Total Stake: 5000 SOL
Stake account is undelegated
Stake Authority: EXU95vqs93yPeCeAU7mPPu6HbRUmTFPEiGug9oCdvQ5F
Withdraw Authority: EXU95vqs93yPeCeAU7mPPu6HbRUmTFPEiGug9oCdvQ5F


Set Stake and Withdraw Authorities
Stake and withdraw authorities can be set when creating an account via the --stake-authority and --withdraw-authority options, or afterward with the solana stake-authorize command. For example, to set a new stake authority, run:

solana stake-authorize <STAKE_ACCOUNT_ADDRESS> \
    --stake-authority <KEYPAIR> --new-stake-authority <PUBKEY> \
    --fee-payer <KEYPAIR>


This will use the existing stake authority <KEYPAIR> to authorize a new stake authority <PUBKEY> on the stake account <STAKE_ACCOUNT_ADDRESS>.

Advanced: Derive Stake Account Addresses
When you delegate stake, you delegate all tokens in the stake account to a single validator. To delegate to multiple validators, you will need multiple stake accounts. Creating a new keypair for each account and managing those addresses can be cumbersome. Fortunately, you can derive stake addresses using the --seed option:

solana create-stake-account --from <KEYPAIR> <STAKE_ACCOUNT_KEYPAIR> --seed <STRING> <AMOUNT> \
    --stake-authority <PUBKEY> --withdraw-authority <PUBKEY> --fee-payer <KEYPAIR>


<STRING> is an arbitrary string up to 32 bytes, but will typically be a number corresponding to which derived account this is. The first account might be "0", then "1", and so on. The public key of <STAKE_ACCOUNT_KEYPAIR> acts as the base address. The command derives a new address from the base address and seed string. To see what stake address the command will derive, use solana create-address-with-seed:

solana create-address-with-seed --from <PUBKEY> <SEED_STRING> STAKE


<PUBKEY> is the public key of the <STAKE_ACCOUNT_KEYPAIR> passed to solana create-stake-account.

The command will output a derived address, which can be used for the <STAKE_ACCOUNT_ADDRESS> argument in staking operations.

Delegate Stake
To delegate your stake to a validator, you will need its vote account address. Find it by querying the cluster for the list of all validators and their vote accounts with the solana validators command:

solana validators

The first column of each row contains the validator's identity and the second is the vote account address. Choose a validator and use its vote account address in solana delegate-stake:

solana delegate-stake --stake-authority <KEYPAIR> <STAKE_ACCOUNT_ADDRESS> <VOTE_ACCOUNT_ADDRESS> \
    --fee-payer <KEYPAIR>


The stake authority <KEYPAIR> authorizes the operation on the account with address <STAKE_ACCOUNT_ADDRESS>. The stake is delegated to the vote account with address <VOTE_ACCOUNT_ADDRESS>.

After delegating stake, use solana stake-account to observe the changes to the stake account:

solana stake-account <STAKE_ACCOUNT_ADDRESS>


You will see new fields "Delegated Stake" and "Delegated Vote Account Address" in the output. The output will look similar to this:

Total Stake: 5000 SOL
Credits Observed: 147462
Delegated Stake: 4999.99771712 SOL
Delegated Vote Account Address: CcaHc2L43ZWjwCHART3oZoJvHLAe9hzT2DJNUpBzoTN1
Stake activates starting from epoch: 42
Stake Authority: EXU95vqs93yPeCeAU7mPPu6HbRUmTFPEiGug9oCdvQ5F
Withdraw Authority: EXU95vqs93yPeCeAU7mPPu6HbRUmTFPEiGug9oCdvQ5F


Deactivate Stake
Once delegated, you can undelegate stake with the solana deactivate-stake command:

solana deactivate-stake --stake-authority <KEYPAIR> <STAKE_ACCOUNT_ADDRESS> \
    --fee-payer <KEYPAIR>


The stake authority <KEYPAIR> authorizes the operation on the account with address <STAKE_ACCOUNT_ADDRESS>.

Note that stake takes several epochs to "cool down". Attempts to delegate stake in the cool down period will fail.

Withdraw Stake
Transfer tokens out of a stake account with the solana withdraw-stake command:

solana withdraw-stake --withdraw-authority <KEYPAIR> <STAKE_ACCOUNT_ADDRESS> <RECIPIENT_ADDRESS> <AMOUNT> \
    --fee-payer <KEYPAIR>


<STAKE_ACCOUNT_ADDRESS> is the existing stake account, the stake authority <KEYPAIR> is the withdraw authority, and <AMOUNT> is the number of tokens to transfer to <RECIPIENT_ADDRESS>.

Split Stake
You may want to delegate stake to additional validators while your existing stake is not eligible for withdrawal. It might not be eligible because it is currently staked, cooling down, or locked up. To transfer tokens from an existing stake account to a new one, use the solana split-stake command:

solana split-stake --stake-authority <KEYPAIR> <STAKE_ACCOUNT_ADDRESS> <NEW_STAKE_ACCOUNT_KEYPAIR> <AMOUNT> \
    --fee-payer <KEYPAIR>


<STAKE_ACCOUNT_ADDRESS> is the existing stake account, the stake authority <KEYPAIR> is the stake authority, <NEW_STAKE_ACCOUNT_KEYPAIR> is the keypair for the new account, and <AMOUNT> is the number of tokens to transfer to the new account.

To split a stake account into a derived account address, use the --seed option. See Derive Stake Account Addresses for details.
use {
    crate::{
        parse_address_lookup_table::parse_address_lookup_table,
        parse_bpf_loader::parse_bpf_upgradeable_loader, parse_config::parse_config,
        parse_nonce::parse_nonce, parse_stake::parse_stake, parse_sysvar::parse_sysvar,
        parse_token::parse_token, parse_vote::parse_vote,
    },
    inflector::Inflector,
    serde_json::Value,
    solana_sdk::{
        address_lookup_table, instruction::InstructionError, pubkey::Pubkey, stake, system_program,
        sysvar, vote,
    },
    std::collections::HashMap,
    thiserror::Error,
};

lazy_static! {
    static ref ADDRESS_LOOKUP_PROGRAM_ID: Pubkey = address_lookup_table::program::id();
    static ref BPF_UPGRADEABLE_LOADER_PROGRAM_ID: Pubkey = solana_sdk::bpf_loader_upgradeable::id();
    static ref CONFIG_PROGRAM_ID: Pubkey = solana_config_program::id();
    static ref STAKE_PROGRAM_ID: Pubkey = stake::program::id();
    static ref SYSTEM_PROGRAM_ID: Pubkey = system_program::id();
    static ref SYSVAR_PROGRAM_ID: Pubkey = sysvar::id();
    static ref VOTE_PROGRAM_ID: Pubkey = vote::program::id();
    pub static ref PARSABLE_PROGRAM_IDS: HashMap<Pubkey, ParsableAccount> = {
        let mut m = HashMap::new();
        m.insert(
            *ADDRESS_LOOKUP_PROGRAM_ID,
            ParsableAccount::AddressLookupTable,
        );
        m.insert(
            *BPF_UPGRADEABLE_LOADER_PROGRAM_ID,
            ParsableAccount::BpfUpgradeableLoader,
        );
        m.insert(*CONFIG_PROGRAM_ID, ParsableAccount::Config);
        m.insert(*SYSTEM_PROGRAM_ID, ParsableAccount::Nonce);
        m.insert(spl_token::id(), ParsableAccount::SplToken);
        m.insert(spl_token_2022::id(), ParsableAccount::SplToken2022);
        m.insert(*STAKE_PROGRAM_ID, ParsableAccount::Stake);
        m.insert(*SYSVAR_PROGRAM_ID, ParsableAccount::Sysvar);
        m.insert(*VOTE_PROGRAM_ID, ParsableAccount::Vote);
        m
    };
}

#[derive(Error, Debug)]
pub enum ParseAccountError {
    #[error("{0:?} account not parsable")]
    AccountNotParsable(ParsableAccount),

    #[error("Program not parsable")]
    ProgramNotParsable,

    #[error("Additional data required to parse: {0}")]
    AdditionalDataMissing(String),

    #[error("Instruction error")]
    InstructionError(#[from] InstructionError),

    #[error("Serde json error")]
    SerdeJsonError(#[from] serde_json::error::Error),
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ParsedAccount {
    pub program: String,
    pub parsed: Value,
    pub space: u64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ParsableAccount {
    AddressLookupTable,
    BpfUpgradeableLoader,
    Config,
    Nonce,
    SplToken,
    SplToken2022,
    Stake,
    Sysvar,
    Vote,
}

#[derive(Clone, Copy, Default)]
pub struct AccountAdditionalData {
    pub spl_token_decimals: Option<u8>,
}

pub fn parse_account_data(
    pubkey: &Pubkey,
    program_id: &Pubkey,
    data: &[u8],
    additional_data: Option<AccountAdditionalData>,
) -> Result<ParsedAccount, ParseAccountError> {
    let program_name = PARSABLE_PROGRAM_IDS
        .get(program_id)
        .ok_or(ParseAccountError::ProgramNotParsable)?;
    let additional_data = additional_data.unwrap_or_default();
    let parsed_json = match program_name {
        ParsableAccount::AddressLookupTable => {
            serde_json::to_value(parse_address_lookup_table(data)?)?
        }
        ParsableAccount::BpfUpgradeableLoader => {
            serde_json::to_value(parse_bpf_upgradeable_loader(data)?)?
        }
        ParsableAccount::Config => serde_json::to_value(parse_config(data, pubkey)?)?,
        ParsableAccount::Nonce => serde_json::to_value(parse_nonce(data)?)?,
        ParsableAccount::SplToken | ParsableAccount::SplToken2022 => {
            serde_json::to_value(parse_token(data, additional_data.spl_token_decimals)?)?
        }
        ParsableAccount::Stake => serde_json::to_value(parse_stake(data)?)?,
        ParsableAccount::Sysvar => serde_json::to_value(parse_sysvar(data, pubkey)?)?,
        ParsableAccount::Vote => serde_json::to_value(parse_vote(data)?)?,
    };
    Ok(ParsedAccount {
        program: format!("{program_name:?}").to_kebab_case(),
        parsed: parsed_json,
        space: data.len() as u64,
    })
}

#[cfg(test)]
mod test {
    use {
        super::*,
        solana_sdk::{
            nonce::{
                state::{Data, Versions},
                State,
            },
            vote::{
                program::id as vote_program_id,
                state::{VoteState, VoteStateVersions},
            },
        },
    };

    #[test]
    fn test_parse_account_data() {
        let account_pubkey = solana_sdk::pubkey::new_rand();
        let other_program = solana_sdk::pubkey::new_rand();
        let data = vec![0; 4];
        assert!(parse_account_data(&account_pubkey, &other_program, &data, None).is_err());

        let vote_state = VoteState::default();
        let mut vote_account_data: Vec<u8> = vec![0; VoteState::size_of()];
        let versioned = VoteStateVersions::new_current(vote_state);
        VoteState::serialize(&versioned, &mut vote_account_data).unwrap();
        let parsed = parse_account_data(
            &account_pubkey,
            &vote_program_id(),
            &vote_account_data,
            None,
        )
        .unwrap();
        assert_eq!(parsed.program, "vote".to_string());
        assert_eq!(parsed.space, VoteState::size_of() as u64);

        let nonce_data = Versions::new(State::Initialized(Data::default()));
        let nonce_account_data = bincode::serialize(&nonce_data).unwrap();
        let parsed = parse_account_data(
            &account_pubkey,
            &system_program::id(),
            &nonce_account_data,
            None,
        )
        .unwrap();
        assert_eq!(parsed.program, "nonce".to_string());
        assert_eq!(parsed.space, State::size() as u64);
    }
}
