//! stakes generator
use crate::{
    address_generator::AddressGenerator,
    unlocks::{UnlockInfo, Unlocks},
};
use solana_sdk::{
    account::Account, clock::Slot, genesis_config::GenesisConfig, native_token::sol_to_lamports,
    pubkey::Pubkey, system_program, timing::years_as_slots,
};
use solana_stake_program::stake_state::{
    create_lockup_stake_account, get_stake_rent_exempt_reserve, Authorized, Lockup,
};

#[derive(Debug)]
pub struct StakerInfo {
    pub name: &'static str,
    pub staker: &'static str,
    pub withdrawer: &'static str,
    pub sol: f64,
    pub custodian: &'static str,
}

// lamports required to run staking operations for one year
//  the staker account needs to be rent exempt *and* carry enough
//  lamports to cover TX fees (delegation) for one year,
//  and we support one delegation per epoch
fn calculate_staker_lamports(genesis_config: &GenesisConfig) -> u64 {
    genesis_config.rent.minimum_balance(0).max(1)
        + genesis_config.fee_calculator.max_lamports_per_signature
            * genesis_config.epoch_schedule.get_epoch(years_as_slots(
                1.0,
                &genesis_config.poh_config.target_tick_duration,
                genesis_config.ticks_per_slot,
            ) as Slot)
}

/// create stake accounts for lamports with at most stake_granularity in each
///  account
pub fn create_and_add_stakes(
    genesis_config: &mut GenesisConfig,
    // information about this staker for this group of stakes
    staker_info: &StakerInfo,
    // description of how the stakes' lockups will expire
    unlock_info: &UnlockInfo,
    // the largest each stake account should be, in lamports
    granularity: u64,
) -> u64 {
    let authorized = Authorized {
        staker: Pubkey::new(&hex::decode(staker_info.staker).expect("hex")),
        withdrawer: Pubkey::new(&hex::decode(staker_info.withdrawer).expect("hex")),
    };
    let custodian = Pubkey::new(&hex::decode(staker_info.custodian).expect("hex"));

    let total_lamports = sol_to_lamports(staker_info.sol);

    let staker_lamports = calculate_staker_lamports(genesis_config);
    let staker_account = (
        authorized.staker,
        Account::new(staker_lamports, 0, &system_program::id()),
    );

    let stakes_lamports = if !genesis_config.accounts.contains(&staker_account) {
        genesis_config.accounts.push(staker_account);

        total_lamports - staker_lamports
    } else {
        total_lamports
    };

    // the staker account needs to be rent exempt *and* carry enough
    //  lamports to cover TX fees (delegation) for one year
    //  as we support one re-delegation per epoch
    let unlocks = Unlocks::new(
        unlock_info.cliff_fraction,
        unlock_info.cliff_years,
        unlock_info.unlocks,
        unlock_info.unlock_years,
        &genesis_config.epoch_schedule,
        &genesis_config.poh_config.target_tick_duration,
        genesis_config.ticks_per_slot,
    );

    let mut address_generator = AddressGenerator::new(&authorized.staker, staker_info.name);

    let stake_rent_exempt_reserve = get_stake_rent_exempt_reserve(&genesis_config.rent);

    for unlock in unlocks {
        let lamports = unlock.amount(stakes_lamports);

        let (granularity, remainder) = if granularity < lamports {
            (granularity, lamports % granularity)
        } else {
            (lamports, 0)
        };

        let lockup = Lockup {
            epoch: unlock.epoch,
            custodian,
        };
        for _ in 0..(lamports / granularity).saturating_sub(1) {
            genesis_config.add_account(
                address_generator.next(),
                create_lockup_stake_account(
                    &authorized,
                    &lockup,
                    &genesis_config.rent,
                    granularity,
                ),
            );
        }
        if remainder <= stake_rent_exempt_reserve {
            genesis_config.add_account(
                address_generator.next(),
                create_lockup_stake_account(
                    &authorized,
                    &lockup,
                    &genesis_config.rent,
                    granularity + remainder,
                ),
            );
        } else {
            genesis_config.add_account(
                address_generator.next(),
                create_lockup_stake_account(
                    &authorized,
                    &lockup,
                    &genesis_config.rent,
                    granularity,
                ),
            );
            genesis_config.add_account(
                address_generator.next(),
                create_lockup_stake_account(&authorized, &lockup, &genesis_config.rent, remainder),
            );
        }
    }
    total_lamports
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::{native_token::lamports_to_sol, rent::Rent};

    fn create_and_check_stakes(
        genesis_config: &mut GenesisConfig,
        staker_info: &StakerInfo,
        unlock_info: &UnlockInfo,
        total_lamports: u64,
        granularity: u64,
        len: usize,
    ) {
        assert!(
            total_lamports
                == create_and_add_stakes(genesis_config, staker_info, unlock_info, granularity)
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
            .all(|(_pubkey, account)| account.lamports <= granularity
                || account.lamports - granularity
                    < get_stake_rent_exempt_reserve(&genesis_config.rent)));
    }

    #[test]
    fn test_create_stakes() {
        // 2 unlocks

        let rent = Rent {
            lamports_per_byte_year: 1,
            exemption_threshold: 1.0,
            ..Rent::default()
        };

        let reserve = get_stake_rent_exempt_reserve(&rent);

        // verify that a small remainder ends up in the last stake
        let granularity = reserve;
        let total_lamports = reserve + reserve * 2 + 1;
        create_and_check_stakes(
            &mut GenesisConfig {
                rent,
                ..GenesisConfig::default()
            },
            &StakerInfo {
                name: "fun",
                staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
                withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
                sol: lamports_to_sol(total_lamports),
                custodian: "0000000000000000000000000000000000000000000000000000000000000000",
            },
            &UnlockInfo {
                cliff_fraction: 0.5,
                cliff_years: 0.5,
                unlocks: 1,
                unlock_years: 0.5,
            },
            total_lamports,
            granularity,
            2 + 1,
        );

        // huge granularity doesn't blow up
        let granularity = std::u64::MAX;
        let total_lamports = reserve + reserve * 2 + 1;
        create_and_check_stakes(
            &mut GenesisConfig {
                rent,
                ..GenesisConfig::default()
            },
            &StakerInfo {
                name: "fun",
                staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
                withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
                sol: lamports_to_sol(total_lamports),
                custodian: "0000000000000000000000000000000000000000000000000000000000000000",
            },
            &UnlockInfo {
                cliff_fraction: 0.5,
                cliff_years: 0.5,
                unlocks: 1,
                unlock_years: 0.5,
            },
            total_lamports,
            granularity,
            2 + 1,
        );

        // exactly reserve as a remainder
        let granularity = reserve * 3;
        let total_lamports = reserve + (granularity + reserve) * 2;
        create_and_check_stakes(
            &mut GenesisConfig {
                rent,
                ..GenesisConfig::default()
            },
            &StakerInfo {
                name: "fun",
                staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
                withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
                sol: lamports_to_sol(total_lamports),
                custodian: "0000000000000000000000000000000000000000000000000000000000000000",
            },
            &UnlockInfo {
                cliff_fraction: 0.5,
                cliff_years: 0.5,
                unlocks: 1,
                unlock_years: 0.5,
            },
            total_lamports,
            granularity,
            4 + 1,
        );
    }
}
