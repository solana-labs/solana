//! validators generator
use solana_sdk::{
    account::Account, genesis_config::GenesisConfig, native_token::sol_to_lamports, pubkey::Pubkey,
    system_program, timing::years_as_slots,
};
use solana_vote_program::vote_state::{self, VoteState};

#[derive(Debug)]
pub struct ValidatorInfo {
    pub name: &'static str,
    pub node: &'static str,
    pub node_sol: f64,
    pub vote: &'static str,
    pub commission: u8,
}

// the node's account needs carry enough
//  lamports to cover TX fees for voting for one year,
//  validators can vote once per slot
fn calculate_voting_fees(genesis_config: &GenesisConfig, years: f64) -> u64 {
    genesis_config.fee_calculator.max_lamports_per_signature
        * years_as_slots(
            years,
            &genesis_config.poh_config.target_tick_duration,
            genesis_config.ticks_per_slot,
        ) as u64
}

/// create and add vote and node id accounts for a validator
pub fn create_and_add_validator(
    genesis_config: &mut GenesisConfig,
    // information about this validator
    validator_info: &ValidatorInfo,
) -> u64 {
    let node: Pubkey = validator_info.node.parse().expect("invalide node");
    let vote: Pubkey = validator_info.vote.parse().expect("invalide vote");
    let node_lamports = sol_to_lamports(validator_info.node_sol);

    // node is the system account from which votes will be issued
    let node_rent_reserve = genesis_config.rent.minimum_balance(0).max(1);
    let node_voting_fees = calculate_voting_fees(genesis_config, 1.0);

    let vote_rent_reserve = VoteState::get_rent_exempt_reserve(&genesis_config.rent).max(1);

    let mut total_lamports = node_voting_fees + vote_rent_reserve + node_lamports;

    genesis_config
        .accounts
        .entry(node)
        .or_insert_with(|| {
            total_lamports += node_rent_reserve;
            Account::new(node_rent_reserve, 0, &system_program::id())
        })
        .lamports += node_voting_fees + node_lamports;

    assert!(
        genesis_config.accounts.get(&vote).is_none(),
        "{} is already in genesis",
        vote
    );

    genesis_config.add_account(
        vote,
        vote_state::create_account(&vote, &node, validator_info.commission, vote_rent_reserve),
    );

    total_lamports
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::rent::Rent;

    fn create_and_check_validators(
        genesis_config: &mut GenesisConfig,
        validator_infos: &[ValidatorInfo],
        total_lamports: u64,
        len: usize,
    ) {
        assert_eq!(
            validator_infos
                .iter()
                .map(|validator_info| create_and_add_validator(genesis_config, validator_info))
                .sum::<u64>(),
            total_lamports
        );
        assert_eq!(genesis_config.accounts.len(), len);
        assert_eq!(
            genesis_config
                .accounts
                .iter()
                .map(|(_pubkey, account)| account.lamports)
                .sum::<u64>(),
            total_lamports,
        );
        assert!(genesis_config
            .accounts
            .iter()
            .all(|(_pubkey, account)| account.lamports
                >= genesis_config.rent.minimum_balance(0).max(1)));
    }

    #[test]
    fn test_create_one_validator() {
        let rent = Rent {
            lamports_per_byte_year: 1,
            exemption_threshold: 1.0,
            ..Rent::default()
        };
        let mut genesis_config = GenesisConfig {
            rent,
            ..GenesisConfig::default()
        };

        let total_lamports = VoteState::get_rent_exempt_reserve(&rent)
            + calculate_voting_fees(&genesis_config, 1.0)
            + rent.minimum_balance(0);

        create_and_check_validators(
            &mut genesis_config,
            &[ValidatorInfo {
                name: "fun",
                node: "AiTDdNHW2vNtHt7PqWMHx3B8cMPRDNgc7kMiLPJM25QC", // random pubkeys
                node_sol: 0.0,
                vote: "77TQYZTHodhnxJcSuVjUvx8GYRCkykPyHtmFTFLjj1Rc",
                commission: 50,
            }],
            total_lamports,
            2,
        );
    }

    #[test]
    fn test_create_one_validator_two_votes() {
        let rent = Rent {
            lamports_per_byte_year: 1,
            exemption_threshold: 1.0,
            ..Rent::default()
        };
        let mut genesis_config = GenesisConfig {
            rent,
            ..GenesisConfig::default()
        };
        let total_lamports = VoteState::get_rent_exempt_reserve(&rent) * 2
            + calculate_voting_fees(&genesis_config, 1.0) * 2 // two vote accounts
            + rent.minimum_balance(0) // one node account
            + sol_to_lamports(1.0); // 2nd vote account ask has SOL

        // weird case, just wanted to verify that the duplicated node account gets double fees
        create_and_check_validators(
            &mut genesis_config,
            &[
                ValidatorInfo {
                    name: "fun",
                    node: "3VTm54dw8w6jTTsPH4BfoV5vo6mF985JAMtNDRYcaGFc", // random pubkeys
                    node_sol: 0.0,
                    vote: "GTKWbUoLw3Bv7Ld92crhyXcEk9zUu3VEKfzeuWJZdnfW",
                    commission: 50,
                },
                ValidatorInfo {
                    name: "unfun",
                    node: "3VTm54dw8w6jTTsPH4BfoV5vo6mF985JAMtNDRYcaGFc", // random pubkeys, same node
                    node_sol: 1.0,
                    vote: "8XrFPRULg98kSm535kFaLV4GMnK5JQSuAymyrCHXsUcy",
                    commission: 50,
                },
            ],
            total_lamports,
            3,
        );
    }

    #[test]
    #[should_panic]
    fn test_vote_collision() {
        let rent = Rent {
            lamports_per_byte_year: 1,
            exemption_threshold: 1.0,
            ..Rent::default()
        };
        let mut genesis_config = GenesisConfig {
            rent,
            ..GenesisConfig::default()
        };

        create_and_check_validators(
            &mut genesis_config,
            &[
                ValidatorInfo {
                    name: "fun",
                    node: "3VTm54dw8w6jTTsPH4BfoV5vo6mF985JAMtNDRYcaGFc", // random pubkeys
                    node_sol: 0.0,
                    vote: "GTKWbUoLw3Bv7Ld92crhyXcEk9zUu3VEKfzeuWJZdnfW",
                    commission: 50,
                },
                ValidatorInfo {
                    name: "unfun",
                    node: "3VTm54dw8w6jTTsPH4BfoV5vo6mF985JAMtNDRYcaGFc", // random pubkeys, same node
                    node_sol: 0.0,
                    vote: "GTKWbUoLw3Bv7Ld92crhyXcEk9zUu3VEKfzeuWJZdnfW", // duplicate vote, bad juju
                    commission: 50,
                },
            ],
            0,
            0,
        );
    }
}
