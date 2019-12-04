use crate::{
    stakes::{create_and_add_stakes, StakerInfo},
    unlocks::UnlockInfo,
    validators::{create_and_add_validator, ValidatorInfo},
};
use solana_sdk::{genesis_config::GenesisConfig, native_token::sol_to_lamports};

// 30 "month" schedule is 1/5th at 6 months
//  1/24 at each 1/12 of a year thereafter
const BATCH_ONE_UNLOCK_INFO: UnlockInfo = UnlockInfo {
    cliff_fraction: 0.2,
    cliff_years: 0.5,
    unlocks: 24,
    unlock_years: 1.0 / 12.0,
};

// 1st batch
const BATCH_ONE_STAKER_INFOS: &[StakerInfo] = &[
    StakerInfo {
        name: "diligent bridge",
        staker: "BwwM47pLHwUgjJXKQKVNiRfGhtPNWfNLH27na2HJQHhd",
        sol: 6_250_000.0,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "four wish",
        staker: "8A6ZEEW2odkqXNjTWHNG6tUk7uj6zCzHueTyEr9pM1tH",
        sol: 10_000_000.0,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "simple friends",
        staker: "D89HyaBmr2WmrTehsfkQrY23wCXcDfsFnN9gMfUXHaDd",
        sol: 1_250_000.0,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "noxious leather",
        staker: "FwPvDpvUmnco1CSfwXQDTbUbuhG5eP7h2vgCKYKVL7at",
        sol: 6_250_000.0,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "worthless direction",
        staker: "4K16iBoC9kAQRT8pUEKeD2h9WEx1zsRgEmJFssXcXmqq",
        sol: 12_500_000.0,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "historical company",
        staker: "rmLpENW4V6QNeEhdJJVxo9Xt99oKgNUFZS4Y4375amW",
        sol: 322_850.0,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "callous money",
        staker: "5kAztE3XtrpeyGZZxckSUt3ZWojNTmph1QSC9S2682z4",
        sol: 5_927_155.25,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "outstanding jump",
        staker: "H6HMVuDR8XCw3EuhLvFG4EciVvGo76Agq1kSBL2ozoDs",
        sol: 625_000.0,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "feeble toes",
        staker: "3sfv8tk5ZSDBWbTkFkvFxCvJUyW5yDJUu6VMJcUARQWq",
        sol: 750_000.0,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "disillusioned deer",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 1_250_000.0,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "unwritten songs",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 4_250_000.0,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "overt dime",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 500_000.0,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "slow committee",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 625_000.0,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "curvy twig",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 625_000.0,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "gamy scissors",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 250_000.0,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "mushy key",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 1_250_000.0,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "marked silver",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 250_000.0,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "free sock",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 625_000.0,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "tremendous meeting",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 1_250_000.0,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "panoramic cloth",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 625_000.0,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "normal kick",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 2_500_000.0,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "unbecoming observation",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 250_000.0,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "cut beginner",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 250_000.0,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "alcoholic button",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 625_000.0,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "old-fashioned clover",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 750_000.0,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "expensive underwear",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 2_500_000.0,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "like dust",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 5_000_000.0,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "rapid straw",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 5_850_000.0,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "windy trousers",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 2_579_350.0,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "dramatic veil",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 3_611_110.50,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "incandescent skin",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 3_000_000.0,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "spiky love",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 3_250_000.0,
        custodian: "11111111111111111111111111111111",
    },
];

// 30 "month" schedule is 1/5th at 6 months
//  1/24 at each 1/12 of a year thereafter
const BATCH_TWO_UNLOCK_INFO: UnlockInfo = UnlockInfo {
    cliff_fraction: 0.2,
    cliff_years: 0.5,
    unlocks: 24,
    unlock_years: 1.0 / 12.0,
};
const BATCH_TWO_STAKER_INFOS: &[StakerInfo] = &[
    // 2nd batch
    StakerInfo {
        name: "macabre note",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 4_000_000.0,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "alcoholic letter",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 4_000_000.0,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "heady trucks",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 4_000_000.0,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "ten support",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 1_000_000.0,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "foregoing middle",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 800_000.0,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "ludicrous destruction",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 4_000_000.0,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "numberless wheel",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 4_000_000.0,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "short powder",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 4_000_000.0,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "cut name",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 4_000_000.0,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "six fly",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 4_000_000.0,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "mindless pickle",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 100_000.0,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "marked rabbit",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 38_741.36,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "jagged doctor",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 711_258.64,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "truthful pollution",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 1_587_300.0,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "unkempt activity",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 2_222_220.0,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "ritzy view",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 40_000.0,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "remarkable plant",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 300_000.0,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "busy value",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 100_000.0,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "imperfect slave",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 222_065.84,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "uneven drawer",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 400_000.0,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "far behavior",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 4_000_000.0,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "abaft memory",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 400_000.0,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "poor glove",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 2_000_000.0,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "strange iron",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 2_000_000.0,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "nonstop rail",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 1_000_000.0,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "milky bait",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 400_000.0,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "wandering start",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 1_200_000.0,
        custodian: "11111111111111111111111111111111",
    },
];
// 30 "month" schedule is 1/5th at 6 months
//  1/24 at each 1/12 of a year thereafter
pub const BATCH_THREE_UNLOCK_INFO: UnlockInfo = UnlockInfo {
    cliff_fraction: 0.2,
    cliff_years: 0.5,
    unlocks: 24,
    unlock_years: 1.0 / 12.0,
};
pub const BATCH_THREE_STAKER_INFOS: &[StakerInfo] = &[
    // 3rd batch
    StakerInfo {
        name: "dusty dress",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 1_212_121.21,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "godly bed",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 151_515.15,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "innocent property",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 227_272.73,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "responsible bikes",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 3_030_303.03,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "learned market",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 3_030_303.03,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "jumpy school",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 303_030.30,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "sticky houses",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 1_515_151.52,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "bustling basketball",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 1_515_152.52,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "ordinary dad",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 606_060.61,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "absurd bat",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 90_909.09,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "cloudy ocean",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 67_945.45,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "black-and-white fold",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 757_575.76,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "stale part",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 45_454.55,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "available health",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 2_797_575.76,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "afraid visitor",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 481_818.18,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "arrogant front",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 151_515.15,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "juvenile zinc",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 151_515.15,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "disturbed box",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 303_030.30,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "disagreeable skate",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 454_545.45,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "miscreant sidewalk",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 75_757.58,
        custodian: "11111111111111111111111111111111",
    },
    StakerInfo {
        name: "shy play",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 303_030.30,
        custodian: "11111111111111111111111111111111",
    },
];

fn add_stakes(
    genesis_config: &mut GenesisConfig,
    staker_infos: &[StakerInfo],
    unlock_info: &UnlockInfo,
    granularity: u64,
) -> u64 {
    staker_infos
        .iter()
        .map(|staker_info| {
            create_and_add_stakes(genesis_config, staker_info, unlock_info, granularity)
        })
        .sum::<u64>()
}

pub const VALIDATOR_INFOS: &[ValidatorInfo] = &[ValidatorInfo {
    name: "aurel@ethereum.ro",
    node: "GeZ5PrJi9muVCJiJAaFBNGoCEdxGEqTp7L2BmT2WTTy1",
    vote: "7ZdRx2EBYoRuPfyeoNbuHodMUXcAQRcC37MUw3kP6akn",
    node_sol: 500.0,
    commission: 0,
}];

fn add_validators(genesis_config: &mut GenesisConfig, validator_infos: &[ValidatorInfo]) -> u64 {
    validator_infos
        .iter()
        .map(|validator_info| create_and_add_validator(genesis_config, validator_info))
        .sum::<u64>()
}

pub fn add_genesis_accounts(genesis_config: &mut GenesisConfig) -> u64 {
    add_stakes(
        genesis_config,
        &BATCH_ONE_STAKER_INFOS,
        &BATCH_ONE_UNLOCK_INFO,
        sol_to_lamports(1_000_000.0),
    ) + add_stakes(
        genesis_config,
        &BATCH_TWO_STAKER_INFOS,
        &BATCH_TWO_UNLOCK_INFO,
        sol_to_lamports(1_000_000.0),
    ) + add_stakes(
        genesis_config,
        &BATCH_THREE_STAKER_INFOS,
        &BATCH_THREE_UNLOCK_INFO,
        sol_to_lamports(1_000_000.0),
    ) + add_validators(genesis_config, &VALIDATOR_INFOS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_genesis_accounts() {
        let mut genesis_config = GenesisConfig::default();

        let issued_lamports = add_genesis_accounts(&mut genesis_config);

        let lamports = genesis_config
            .accounts
            .iter()
            .map(|(_, account)| account.lamports)
            .sum::<u64>();

        assert_eq!(issued_lamports, lamports);
    }
}
