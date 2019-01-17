//! Vote program
//! Receive and processes votes from validators

use bincode::{deserialize, serialize_into};
use log::*;
use solana_sdk::account::{Account, KeyedAccount};
use solana_sdk::native_program::ProgramError;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::solana_entrypoint;
use solana_sdk::vote_program::*;
use solana_sdk::weighted_election::WeightedElection;
use std::collections::{HashMap, VecDeque};

fn initialize_election_account(
    account: &mut Account,
    weights: HashMap<Pubkey, u64>,
) -> Result<(), ProgramError> {
    let election = WeightedElection::new(weights);
    serialize_into(&mut account.userdata[..], &election).map_err(|_| ProgramError::UserdataTooSmall)
}

fn propose_block(
    accounts: &mut [KeyedAccount],
    description: BlockDescription,
) -> Result<(), ProgramError> {
    if accounts.len() < 2 {
        return Err(ProgramError::GenericError);
    }

    if accounts[0].signer_key().is_none() {
        return Err(ProgramError::AssignOfUnownedAccount);
    }

    initialize_election_account(&mut accounts[1].account, description.weights)
}

fn vote(accounts: &mut [KeyedAccount]) -> Result<(), ProgramError> {
    if accounts.len() < 2 {
        return Err(ProgramError::GenericError);
    }

    let voter_id = match accounts[0].signer_key() {
        None => {
            return Err(ProgramError::AssignOfUnownedAccount);
        }
        Some(id) => id,
    };

    let mut election: WeightedElection =
        deserialize(&accounts[1].account.userdata).map_err(|_| ProgramError::InvalidUserdata)?;

    election
        .vote(&voter_id)
        .map_err(|_| ProgramError::GenericError)?;

    serialize_into(&mut accounts[1].account.userdata[..], &election).unwrap();
    Ok(())
}

solana_entrypoint!(entrypoint);
fn entrypoint(
    _program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    data: &[u8],
    _tick_height: u64,
) -> Result<(), ProgramError> {
    solana_logger::setup();

    trace!("process_instruction: {:?}", data);
    trace!("keyed_accounts: {:?}", keyed_accounts);

    // all vote instructions require that accounts_keys[0] be a signer
    if keyed_accounts[0].signer_key().is_none() {
        error!("account[0] is unsigned");
        Err(ProgramError::InvalidArgument)?;
    }

    match deserialize(data).map_err(|_| ProgramError::InvalidUserdata)? {
        VoteInstruction::RegisterAccount => {
            if !check_id(&keyed_accounts[1].account.owner) {
                error!("account[1] is not assigned to the VOTE_PROGRAM");
                Err(ProgramError::InvalidArgument)?;
            }

            // TODO: a single validator could register multiple "vote accounts"
            // which would clutter the "accounts" structure. See github issue 1654.
            let vote_state = VoteProgram {
                votes: VecDeque::new(),
                node_id: *keyed_accounts[0].signer_key().unwrap(),
            };

            vote_state.serialize(&mut keyed_accounts[1].account.userdata)?;

            Ok(())
        }
        VoteInstruction::NewVote(vote) => {
            if !check_id(&keyed_accounts[0].account.owner) {
                error!("account[0] is not assigned to the VOTE_PROGRAM");
                Err(ProgramError::InvalidArgument)?;
            }
            debug!("{:?} by {}", vote, keyed_accounts[0].signer_key().unwrap());
            solana_metrics::submit(
                solana_metrics::influxdb::Point::new("vote-native")
                    .add_field("count", solana_metrics::influxdb::Value::Integer(1))
                    .to_owned(),
            );

            let mut vote_state = VoteProgram::deserialize(&keyed_accounts[0].account.userdata)?;

            // TODO: Integrity checks
            // a) Verify the vote's bank hash matches what is expected
            // b) Verify vote is older than previous votes

            // Only keep around the most recent MAX_VOTE_HISTORY votes
            if vote_state.votes.len() == MAX_VOTE_HISTORY {
                vote_state.votes.pop_front();
            }

            vote_state.votes.push_back(vote);
            vote_state.serialize(&mut keyed_accounts[0].account.userdata)?;

            Ok(())
        }
        VoteInstruction::ProposeBlock(description) => propose_block(keyed_accounts, description),
        VoteInstruction::Vote => vote(keyed_accounts),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::hash::Hash;
    use solana_sdk::signature::{Keypair, KeypairUtil};

    fn create_election_account(tokens: u64, num_voters: usize) -> Account {
        let space = WeightedElection::serialized_size(num_voters);
        Account::new(tokens, space, Pubkey::default())
    }

    fn create_initialized_election_account(tokens: u64, weights: HashMap<Pubkey, u64>) -> Account {
        let mut account = create_election_account(tokens, weights.len());
        initialize_election_account(&mut account, weights).unwrap();
        account
    }

    fn propose_block_and_deserialize(
        keyed_accounts: &mut [KeyedAccount],
        desc: BlockDescription,
    ) -> Result<WeightedElection, ProgramError> {
        propose_block(keyed_accounts, desc)?;
        let election = deserialize(&keyed_accounts[1].account.userdata).unwrap();
        Ok(election)
    }

    fn vote_and_deserialize(
        keyed_accounts: &mut [KeyedAccount],
    ) -> Result<WeightedElection, ProgramError> {
        vote(keyed_accounts)?;
        let election = deserialize(&keyed_accounts[1].account.userdata).unwrap();
        Ok(election)
    }

    #[test]
    fn test_propose_block() {
        let leader_id = Keypair::new().pubkey();
        let mut leader_account = Account::new(100, 0, Pubkey::default());

        let mut weights = HashMap::new();
        weights.insert(leader_id, 1);
        let election_id = Keypair::new().pubkey();
        let mut election_account = create_election_account(100, weights.len());

        let mut keyed_accounts = [
            KeyedAccount::new(&leader_id, true, &mut leader_account),
            KeyedAccount::new(&election_id, false, &mut election_account),
        ];

        let desc = BlockDescription::new(0, Hash::default(), Hash::default(), weights);
        let election = propose_block_and_deserialize(&mut keyed_accounts, desc).unwrap();
        assert_eq!(election.total_weight(), 1);
    }

    #[test]
    fn test_propose_block_missing_accounts() {
        let leader_id = Keypair::new().pubkey();
        let mut leader_account = Account::new(100, 0, Pubkey::default());

        let mut keyed_accounts = [KeyedAccount::new(&leader_id, true, &mut leader_account)]; // <--- Missing account

        let weights = HashMap::new();
        let desc = BlockDescription::new(0, Hash::default(), Hash::default(), weights);
        assert_eq!(
            propose_block_and_deserialize(&mut keyed_accounts, desc),
            Err(ProgramError::GenericError)
        );
    }

    #[test]
    fn test_propose_block_unsigned() {
        let leader_id = Keypair::new().pubkey();
        let mut leader_account = Account::new(100, 0, Pubkey::default());

        let weights = HashMap::new();
        let election_id = Keypair::new().pubkey();
        let mut election_account = create_election_account(100, weights.len());

        let mut keyed_accounts = [
            KeyedAccount::new(&leader_id, false, &mut leader_account), // <--- attack!
            KeyedAccount::new(&election_id, false, &mut election_account),
        ];

        let desc = BlockDescription::new(0, Hash::default(), Hash::default(), weights);
        assert_eq!(
            propose_block_and_deserialize(&mut keyed_accounts, desc),
            Err(ProgramError::AssignOfUnownedAccount)
        );
    }

    #[test]
    fn test_propose_block_with_inadequate_space() {
        let leader_id = Keypair::new().pubkey();
        let mut leader_account = Account::new(100, 0, Pubkey::default());

        let mut weights = HashMap::new();
        weights.insert(leader_id, 1);

        let election_id = Keypair::new().pubkey();
        let mut election_account = create_election_account(100, 0); // <---- Oops, zero is less than weights.len()

        let mut keyed_accounts = [
            KeyedAccount::new(&leader_id, true, &mut leader_account),
            KeyedAccount::new(&election_id, false, &mut election_account),
        ];

        let desc = BlockDescription::new(0, Hash::default(), Hash::default(), weights);
        assert_eq!(
            propose_block_and_deserialize(&mut keyed_accounts, desc),
            Err(ProgramError::UserdataTooSmall)
        );
    }

    #[test]
    fn test_vote() {
        let voter_id = Keypair::new().pubkey();
        let mut voter_account = Account::new(100, 0, Pubkey::default());

        let election_id = Keypair::new().pubkey();
        let mut weights = HashMap::new();
        weights.insert(voter_id, 1);

        let mut election_account = create_initialized_election_account(100, weights);

        let mut keyed_accounts = [
            KeyedAccount::new(&voter_id, true, &mut voter_account),
            KeyedAccount::new(&election_id, false, &mut election_account),
        ];

        let election = vote_and_deserialize(&mut keyed_accounts).unwrap();
        assert_eq!(election.voted_weight(), 1);
    }

    #[test]
    fn test_vote_unsigned() {
        let voter_id = Keypair::new().pubkey();
        let mut voter_account = Account::new(100, 0, Pubkey::default());

        let election_id = Keypair::new().pubkey();
        let mut weights = HashMap::new();
        weights.insert(voter_id, 1);

        let mut election_account = create_initialized_election_account(100, weights);

        let mut keyed_accounts = [
            KeyedAccount::new(&voter_id, false, &mut voter_account), // <--- attack!
            KeyedAccount::new(&election_id, false, &mut election_account),
        ];

        assert_eq!(
            vote_and_deserialize(&mut keyed_accounts),
            Err(ProgramError::AssignOfUnownedAccount)
        );
    }

    #[test]
    fn test_vote_missing_accounts() {
        let voter_id = Keypair::new().pubkey();
        let mut voter_account = Account::new(100, 0, Pubkey::default());

        let mut keyed_accounts = [KeyedAccount::new(&voter_id, true, &mut voter_account)]; // <--- Missing account

        assert_eq!(
            vote_and_deserialize(&mut keyed_accounts),
            Err(ProgramError::GenericError)
        );
    }

    #[test]
    fn test_vote_corrupt_data() {
        let voter_id = Keypair::new().pubkey();
        let mut voter_account = Account::new(100, 0, Pubkey::default());

        let election_id = Keypair::new().pubkey();
        let mut weights = HashMap::new();
        weights.insert(voter_id, 1);

        let mut election_account = create_initialized_election_account(100, weights);

        let mut keyed_accounts = [
            KeyedAccount::new(&election_id, true, &mut election_account), // <--- Oops, reversed account order
            KeyedAccount::new(&voter_id, false, &mut voter_account),
        ];

        assert_eq!(
            vote_and_deserialize(&mut keyed_accounts),
            Err(ProgramError::InvalidUserdata)
        );
    }

    #[test]
    fn test_vote_id_not_found() {
        let voter_id = Keypair::new().pubkey();
        let bogus_id = Keypair::new().pubkey();
        let mut bogus_account = Account::new(100, 0, Pubkey::default());

        let election_id = Keypair::new().pubkey();
        let mut weights = HashMap::new();
        weights.insert(voter_id, 1);

        let mut election_account = create_initialized_election_account(100, weights);

        let mut keyed_accounts = [
            KeyedAccount::new(&bogus_id, true, &mut bogus_account),
            KeyedAccount::new(&election_id, false, &mut election_account),
        ];

        assert_eq!(
            vote_and_deserialize(&mut keyed_accounts),
            Err(ProgramError::GenericError)
        );
    }
}
