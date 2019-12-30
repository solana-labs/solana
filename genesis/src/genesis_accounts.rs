use crate::{
    stakes::{create_and_add_stakes, StakerInfo},
    unlocks::UnlockInfo,
    validators::{create_and_add_validator, ValidatorInfo},
};
use solana_sdk::{genesis_config::GenesisConfig, native_token::LAMPORTS_PER_SOL};

// 30 month schedule is 1/5th every 6 months for 30 months
const UNLOCKS_BY_FIFTHS_FOR_30_MONTHS: UnlockInfo = UnlockInfo {
    cliff_fraction: 0.2,
    cliff_years: 0.5,
    unlocks: 4,
    unlock_years: 0.5,
    custodian: "6LnFgiECFQKUcxNYDvUBMxgjeGQzzy4kgxGhantoxfUe",
};

// 60 month schedule is 1/10th every 6 months for 60 months
const UNLOCKS_BY_TENTHS_FOR_60_MONTHS: UnlockInfo = UnlockInfo {
    cliff_fraction: 0.1,
    cliff_years: 0.5,
    unlocks: 9,
    unlock_years: 0.5,
    custodian: "11111111111111111111111111111111",
};

// no lockups
const UNLOCKS_ALL_DAY_ZERO: UnlockInfo = UnlockInfo {
    cliff_fraction: 1.0,
    cliff_years: 0.0,
    unlocks: 0,
    unlock_years: 0.0,
    custodian: "11111111111111111111111111111111",
};

pub const BATCH_FOUR_STAKER_INFOS: &[StakerInfo] = &[
    StakerInfo {
        name: "impossible pizza",
        staker: "CDtJpwRSiPRDGeKrvymWQKM7JY9M3hU7iimEKBDxZyoP",
        lamports: 5_000_000 * LAMPORTS_PER_SOL,
    },
    StakerInfo {
        name: "wretched texture",
        staker: "HbENu65qjWLEB5TrMouSSWLq9mbtGx2bvfhPjk2FpYek",
        lamports: 225_000 * LAMPORTS_PER_SOL,
    },
    StakerInfo {
        name: "nutritious examination",
        staker: "C9CfFpmLDsQsz6wt7MrrZquNB5oS4QkpJkmDAiboVEZZ",
        lamports: 5_000_000 * LAMPORTS_PER_SOL,
    },
    StakerInfo {
        name: "tidy impression",
        staker: "6ne6Rbag4FAnop1KNgVdM1SEHnJEysHSWyqvRpFrzaig",
        lamports: 5_000_000 * LAMPORTS_PER_SOL,
    },
    StakerInfo {
        name: "unbecoming silver",
        staker: "42yapY7Vrs5jqht9TCKZsPoyb4vDFYcPfRkqAP85NSAQ",
        lamports: 28_800 * LAMPORTS_PER_SOL,
    },
    StakerInfo {
        name: "dramatic treatment",
        staker: "GTyawCMwt3kMb51AgDtfdp97mDot7jNwc8ifuS9qqANg",
        lamports: 1_205_602 * LAMPORTS_PER_SOL,
    },
    StakerInfo {
        name: "angry noise",
        staker: "Fqxs9MhqjKuMq6YwjBG4ktEapuZQ3kj19mpuHLZKtkg9",
        lamports: 5_000_000 * LAMPORTS_PER_SOL,
    },
    StakerInfo {
        name: "hard cousin",
        staker: "9MYDzj7QuAX9QAK7da1GhzPB4gA3qbPNWsW3MMSZobru",
        lamports: 5_000_000 * LAMPORTS_PER_SOL,
    },
    StakerInfo {
        name: "inexpensive uncle",
        staker: "E4DLNkmdL34ejA48ApfPDoFVuD9XWAFqi8bXzBGRhKst",
        lamports: 300_000 * LAMPORTS_PER_SOL,
    },
    StakerInfo {
        name: "lopsided skill",
        staker: "8cV7zCTF5UMrZakZXiL2Jw5uY3ms2Wz4twzFXEY9Kge2",
        lamports: 5_000_000 * LAMPORTS_PER_SOL,
    },
    StakerInfo {
        name: "red snake",
        staker: "JBGnGdLyo7V2z9hz51mnnbyDp9sBACtw5WYH9YRG8n7e",
        lamports: 3_655_292 * LAMPORTS_PER_SOL,
    },
    StakerInfo {
        name: "hellish money",
        staker: "CqKdQ57mBj2mKcAbpjWc28Ls7yXzBXboxSTCRWocmUVj",
        lamports: 200_000 * LAMPORTS_PER_SOL,
    },
    StakerInfo {
        name: "full grape",
        staker: "2SCJKvh7wWo32PtfUZdVZQ84WnMWoUpF4WTm6ZxcCJ15",
        lamports: 450_000 * LAMPORTS_PER_SOL,
    },
    StakerInfo {
        name: "nice ghost",
        staker: "FeumxB3gfzrVQzABBiha8AacKPY3Rf4BTFSh2aZWHqR8",
        lamports: 650_000 * LAMPORTS_PER_SOL,
    },
    StakerInfo {
        name: "jolly year",
        staker: "HBwFWNGPVZgkf3yqUKxuAds5aANGWX62LzUFvZVCWLdJ",
        lamports: 5_000_000 * LAMPORTS_PER_SOL,
    },
    StakerInfo {
        name: "typical initiative",
        staker: "3JMz3kaDUZEVK2JVjRqwERGMp7LbWbgUjAFBb42qxoHb",
        lamports: 5_000_000 * LAMPORTS_PER_SOL,
    },
    StakerInfo {
        name: "deserted window",
        staker: "XTeBBZextvHkoRqDF8yb4hihjcraKQDwTEXhzjd8fip",
        lamports: 3_655_292 * LAMPORTS_PER_SOL,
    },
    StakerInfo {
        name: "eight nation",
        staker: "E5bSU5ywqPiz3ije89ef5gaEC7jy81BAc72Zeb9MqeHY",
        lamports: 103_519 * LAMPORTS_PER_SOL,
    },
    StakerInfo {
        name: "earsplitting meaning",
        staker: "4ZemkSoE75RFE1SVLnnmHcaNWT4qN8KFrKP2wAYfv8CB",
        lamports: 5_000_000 * LAMPORTS_PER_SOL,
    },
    StakerInfo {
        name: "alike cheese",
        staker: "72BGEwYee5txFonmpEarTEKCZVN2UxcSUgdphdhcx3V",
        lamports: 3_880_295 * LAMPORTS_PER_SOL,
    },
    StakerInfo {
        name: "noisy honey",
        staker: "DRp1Scyn4yJZQfMAdQew2x8RtvRmsNELN37JTK5Xvzgn",
        lamports: 5_000_000 * LAMPORTS_PER_SOL,
    },
];

pub const FOUNDATION_STAKER_INFOS: &[StakerInfo] = &[
    StakerInfo {
        name: "lyrical supermarket",
        staker: "GRZwoJGisLTszcxtWpeREJ98EGg8pZewhbtcrikoU7b3",
        lamports: 5_000_000 * LAMPORTS_PER_SOL,
    },
    StakerInfo {
        name: "frequent description",
        staker: "J51tinoLdmEdUR27LUVymrb2LB3xQo1aSHSgmbSGdj58",
        lamports: 57_500_000 * LAMPORTS_PER_SOL,
    },
];

pub const GRANTS_STAKER_INFOS: &[StakerInfo] = &[
    StakerInfo {
        name: "rightful agreement",
        staker: "DNaKiBwwbbqk1wVoC5AQxWQbuDhvaDVbAtXzsVos9mrc",
        lamports: 5_000_000 * LAMPORTS_PER_SOL,
    },
    StakerInfo {
        name: "tasty location",
        staker: "HvXQPXAijjG1vnQs6HXVtUUtFVzi5HNgXV9LGnHvYF85",
        lamports: 15_000_000 * LAMPORTS_PER_SOL,
    },
];

pub const COMMUNITY_STAKER_INFOS: &[StakerInfo] = &[
    StakerInfo {
        name: "shrill charity",
        staker: "BzuqQFnu7oNUeok9ZoJezpqu2vZJU7XR1PxVLkk6wwUD",
        lamports: 5_000_000 * LAMPORTS_PER_SOL,
    },
    StakerInfo {
        name: "legal gate",
        staker: "FwMbkDZUb78aiMWhZY4BEroAcqmnrXZV77nwrg71C57d",
        lamports: 16_086_641 * LAMPORTS_PER_SOL,
    },
    StakerInfo {
        name: "cluttered complaint",
        staker: "4h1rt2ic4AXwG7p3Qqhw57EMDD4c3tLYb5J3QstGA2p5",
        lamports: 153_333_633 * LAMPORTS_PER_SOL + 41 * LAMPORTS_PER_SOL / 100,
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

pub const VALIDATOR_INFOS: &[ValidatorInfo] = &[
    ValidatorInfo {
        name: "01Node",
        node: "5n8KCdzqtvTnhkvCrFR7errH6ZUp11kL97r2awXkfzFe",
        vote: "4uYMbY5Ae5ZSRNxQ3RWVyXS9rzW7E3AMZYHuUEotxu6K",
        node_lamports: 500 * LAMPORTS_PER_SOL,
        commission: 0,
    },
    ValidatorInfo {
        name: "Bison Trails",
        node: "7suRNpX7bJsXphHJtBv4ZsLjJZ1dTGeX256pLqJZdEAm",
        vote: "DfirEZ9Up1xbE7sQji9UwtcRGe5uCcRqQtnaGpha5KNY",
        node_lamports: 500 * LAMPORTS_PER_SOL,
        commission: 0,
    },
    ValidatorInfo {
        name: "ChainFlow",
        node: "2te46rxywMdCNdkvjumiBBPQoVczJFxhxEaxFavQNqe3",
        vote: "8bRCnytB7bySmqxodNGbZuUAtncKkB8T733DD1Dm9WMb",
        node_lamports: 500 * LAMPORTS_PER_SOL,
        commission: 0,
    },
    ValidatorInfo {
        name: "ChorusOne",
        node: "ChorusXqjLC2NbiStKR6k9WoD7wu6TVTtFG8qCL5XBVa",
        vote: "ChorusvBuPwukqgDvYfWtEg8j4T1NcMgSTQ4b1UbAwgQ",
        node_lamports: 500 * LAMPORTS_PER_SOL,
        commission: 0,
    },
    ValidatorInfo {
        name: "Dokia Capital",
        node: "GeZ5PrJi9muVCJiJAaFBNGoCEdxGEqTp7L2BmT2WTTy1",
        vote: "7ZdRx2EBYoRuPfyeoNbuHodMUXcAQRcC37MUw3kP6akn",
        node_lamports: 500 * LAMPORTS_PER_SOL,
        commission: 0,
    },
    ValidatorInfo {
        name: "Forbole",
        node: "Fe5sLQAAT7RBT8mcH1AAGCbExJQcYxcwXvp1GjrGbvxs",
        vote: "Dr8MkZZuvZVQJFKtjShZYEfg6n93sc1GxevqLnGss7FW",
        node_lamports: 500 * LAMPORTS_PER_SOL,
        commission: 0,
    },
    ValidatorInfo {
        name: "P2P.ORG - Secure Non-custodial Staking",
        node: "44e8VyWoyZSE2oYHxMHMedAiHkGJqJgPd3tdt6iKoAFL",
        vote: "BwwpzEpo1wzgV9N1987ntgNG6jLt3C9532C68pswT7Gp",
        node_lamports: 500 * LAMPORTS_PER_SOL,
        commission: 0,
    },
    ValidatorInfo {
        name: "RockX",
        node: "Ez4iUU87ViJLCnmSy1t1Ti3DLoysFXiBseNfnRfoehyY",
        vote: "GUdGALCHQBeqkNc2ZAht3tBXab1N5u9qJC3PAzpL54r7",
        node_lamports: 500 * LAMPORTS_PER_SOL,
        commission: 0,
    },
    ValidatorInfo {
        name: "Stake Capital",
        node: "HavuVVDXXsJqMzPwQ4KcF5kFm2xqjbChhyi1bgGeCQif",
        vote: "HswPkKj1xoLLmpM8t1vy5Pbi8zYYUs9ZawswvofKsFo1",
        node_lamports: 500 * LAMPORTS_PER_SOL,
        commission: 0,
    },
    ValidatorInfo {
        name: "Staking Facilities",
        node: "pbAxyqHHPMwgEjv8kmjGxysk9rhNtN7q22eAjReq6Hj",
        vote: "4VZ3pJX19PpuGjoSB1qeN9sVQfrqgLVNg16is37adiFp",
        node_lamports: 500 * LAMPORTS_PER_SOL,
        commission: 0,
    },
];

fn add_validators(genesis_config: &mut GenesisConfig, validator_infos: &[ValidatorInfo]) -> u64 {
    validator_infos
        .iter()
        .map(|validator_info| create_and_add_validator(genesis_config, validator_info))
        .sum::<u64>()
}

pub fn add_genesis_accounts(genesis_config: &mut GenesisConfig, mut issued_lamports: u64) {
    // add_stakes() and add_validators() award tokens for rent exemption and
    //  to cover an initial transfer-free period of the network

    issued_lamports += add_stakes(
        genesis_config,
        &BATCH_FOUR_STAKER_INFOS,
        &UNLOCKS_BY_FIFTHS_FOR_30_MONTHS,
        1_000_000 * LAMPORTS_PER_SOL,
    ) + add_stakes(
        genesis_config,
        &FOUNDATION_STAKER_INFOS,
        &UNLOCKS_BY_TENTHS_FOR_60_MONTHS,
        1_000_000 * LAMPORTS_PER_SOL,
    ) + add_stakes(
        genesis_config,
        &GRANTS_STAKER_INFOS,
        &UNLOCKS_BY_TENTHS_FOR_60_MONTHS,
        1_000_000 * LAMPORTS_PER_SOL,
    ) + add_stakes(
        genesis_config,
        &COMMUNITY_STAKER_INFOS,
        &UNLOCKS_ALL_DAY_ZERO,
        1_000_000 * LAMPORTS_PER_SOL,
    ) + add_validators(genesis_config, &VALIDATOR_INFOS);

    // "one thanks" (community pool) gets 500_000_000SOL (total) - above distributions
    create_and_add_stakes(
        genesis_config,
        &StakerInfo {
            name: "one thanks",
            staker: "3b7akieYUyCgz3Cwt5sTSErMWjg8NEygD6mbGjhGkduB",
            lamports: 500_000_000 * LAMPORTS_PER_SOL - issued_lamports,
        },
        &UNLOCKS_ALL_DAY_ZERO,
        1_000_000 * LAMPORTS_PER_SOL,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_genesis_accounts() {
        let mut genesis_config = GenesisConfig::default();

        add_genesis_accounts(&mut genesis_config, 0);

        let lamports = genesis_config
            .accounts
            .iter()
            .map(|(_, account)| account.lamports)
            .sum::<u64>();

        assert_eq!(500_000_000 * LAMPORTS_PER_SOL, lamports);
    }
}
