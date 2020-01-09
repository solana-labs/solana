//! validators generator
use solana_sdk::{
    account::Account, genesis_config::GenesisConfig, pubkey::Pubkey, system_program,
    timing::years_as_slots,
};

#[derive(Debug)]
pub struct ValidatorInfo {
    pub name: &'static str,
    pub node: &'static str,
    pub node_lamports: u64,
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

/// create accounts for a validator
pub fn create_and_add_validator(
    genesis_config: &mut GenesisConfig,
    // information about this validator
    validator_info: &ValidatorInfo,
) -> u64 {
    let node: Pubkey = validator_info.node.parse().expect("invalid node");

    // node is the system account from which votes will be issued
    let node_rent_reserve = genesis_config.rent.minimum_balance(0).max(1);
    let node_voting_fees = calculate_voting_fees(genesis_config, 1.0);

    let mut total_lamports = node_voting_fees + validator_info.node_lamports;

    genesis_config
        .accounts
        .entry(node)
        .or_insert_with(|| {
            total_lamports += node_rent_reserve;
            Account::new(node_rent_reserve, 0, &system_program::id())
        })
        .lamports += node_voting_fees + validator_info.node_lamports;

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

        let total_lamports = calculate_voting_fees(&genesis_config, 1.0) + rent.minimum_balance(0);

        create_and_check_validators(
            &mut genesis_config,
            &[ValidatorInfo {
                name: "fun",
                node: "AiTDdNHW2vNtHt7PqWMHx3B8cMPRDNgc7kMiLPJM25QC", // random pubkey
                node_lamports: 0,
            }],
            total_lamports,
            1,
        );
    }
}
