use crate::{
    stakes::{create_and_add_stakes, StakerInfo},
    unlocks::UnlockInfo,
};
use solana_sdk::{genesis_config::GenesisConfig, native_token::LAMPORTS_PER_SOL};

// 9 month schedule is 100% after 9 months
const UNLOCKS_ALL_AT_9_MONTHS: UnlockInfo = UnlockInfo {
    cliff_fraction: 1.0,
    cliff_years: 0.75,
    unlocks: 0,
    unlock_years: 0.0,
    custodian: "Mc5XB47H3DKJHym5RLa9mPzWv5snERsF3KNv5AauXK8",
};

// 9 month schedule is 50% after 9 months, then monthly for 2 years
const UNLOCKS_HALF_AT_9_MONTHS: UnlockInfo = UnlockInfo {
    cliff_fraction: 0.5,
    cliff_years: 0.75,
    unlocks: 24,
    unlock_years: 2.0,
    custodian: "Mc5XB47H3DKJHym5RLa9mPzWv5snERsF3KNv5AauXK8",
};

// no lockups
const UNLOCKS_ALL_DAY_ZERO: UnlockInfo = UnlockInfo {
    cliff_fraction: 1.0,
    cliff_years: 0.0,
    unlocks: 0,
    unlock_years: 0.0,
    custodian: "Mc5XB47H3DKJHym5RLa9mPzWv5snERsF3KNv5AauXK8",
};

pub const CREATOR_STAKER_INFOS: &[StakerInfo] = &[
    StakerInfo {
        name: "impossible pizza",
        staker: "uE3TVEffRp69mrgknYr71M18GDqL7GxCNGYYRjb3oUt",
        lamports: 5_000_000 * LAMPORTS_PER_SOL,
        withdrawer: Some("59SLqk4ete5QttM1WmjfMA7uNJnJVFLQqXJSy9rvuj7c"),
    },
    StakerInfo {
        name: "nutritious examination",
        staker: "9noVEZreMmgQvE8iyKmxy7CGTJ2enELyuJ1qxFtXrfJB",
        lamports: 5_000_000 * LAMPORTS_PER_SOL,
        withdrawer: Some("ERnx3Csgu3LjrGGrCeCUZzuHguRu6XabT1kufSB1NDWi"),
    },
    StakerInfo {
        name: "tidy impression",
        staker: "BU7LA4kYvicfPCp22EM2Tth3eaeWAXYo6yCgWXQFJ42z",
        lamports: 5_000_000 * LAMPORTS_PER_SOL,
        withdrawer: Some("5eKcGy7ZCPJdQSQGVnfmT7kGz6MKPMKaNaMEYJbmwhuT"),
    },
    StakerInfo {
        name: "dramatic treatment",
        staker: "BrNFrFeuev8TosKhRe2kvVZTYrcUuYaqCfptWutxs17B",
        lamports: 1_205_602 * LAMPORTS_PER_SOL,
        withdrawer: Some("2pKqwFKfKj2nGrknPNDSP8vXGYrgAjd28fT6yLew8sT3"),
    },
    StakerInfo {
        name: "angry noise",
        staker: "Fqxs9MhqjKuMq6YwjBG4ktEapuZQ3kj19mpuHLZKtkg9",
        lamports: 5_000_000 * LAMPORTS_PER_SOL,
        withdrawer: None,
    },
    StakerInfo {
        name: "hard cousin",
        staker: "AyZb3xrZE8wnS6gYBdsJg5v8CjyrX2ZGXU2zMakCFyYd",
        lamports: 5_000_000 * LAMPORTS_PER_SOL,
        withdrawer: Some("9j3WzBSZRHrD2DbzFTUVVi81QX6boVvUTpGWcSiMwD5W"),
    },
    StakerInfo {
        name: "lopsided skill",
        staker: "7SbpY8LmZUb5XRqDbyoreUrSVVV9c39wkpEz81kEAXu5",
        lamports: 5_000_000 * LAMPORTS_PER_SOL,
        withdrawer: Some("EJyZGbQ1PmpcWxfqGME6SUNHfurh1zggDqCT7rV9xLzL"),
    },
    StakerInfo {
        name: "red snake",
        staker: "C9CfFpmLDsQsz6wt7MrrZquNB5oS4QkpJkmDAiboVEZZ",
        lamports: 3_655_292 * LAMPORTS_PER_SOL,
        withdrawer: Some("JBGnGdLyo7V2z9hz51mnnbyDp9sBACtw5WYH9YRG8n7e"),
    },
    StakerInfo {
        name: "jolly year",
        staker: "5WbxKiW7bghkr8JN6ZAv2TQt4PfJFvtuqNaN8gyQ5UzU",
        lamports: 5_000_000 * LAMPORTS_PER_SOL,
        withdrawer: Some("43XAfG3AFiF1ockdh7xp91fpFyZkbWSZq9ZFBCGUVV41"),
    },
    StakerInfo {
        name: "typical initiative",
        staker: "Gc8XnHU6Nnriwt9RbEwi7PTosx4YanLyXak9GTbB8VaH",
        lamports: 5_000_000 * LAMPORTS_PER_SOL,
        withdrawer: Some("7s2GVwFo8VSrCwX9Tztt42ueiEaUtJ6zCEHU8XGvuf5E"),
    },
    StakerInfo {
        name: "deserted window",
        staker: "AMmYEynkd78uNTZDFFrMw6NKjWTgqW7M8EFjvajk23VR",
        lamports: 3_655_292 * LAMPORTS_PER_SOL,
        withdrawer: Some("23PJYLS1WFLqhXnXq2Hobc17DbvZaoinoTZYLyGRT8E2"),
    },
    StakerInfo {
        name: "eight nation",
        staker: "E5bSU5ywqPiz3ije89ef5gaEC7jy81BAc72Zeb9MqeHY",
        lamports: 103_519 * LAMPORTS_PER_SOL,
        withdrawer: Some("BEH58KnvhU8WNyFto21ZXQMfkHKHY2vK7CiTLLCR1MWJ"),
    },
    StakerInfo {
        name: "earsplitting meaning",
        staker: "GYitoBY53E9awc56NWHJ2kxMwj4do5GSmvTRowjGaRDw",
        lamports: 5_000_000 * LAMPORTS_PER_SOL,
        withdrawer: Some("jXMEkVQQpoqebVMGN7DfpvdRLwJDEkoVNrwPVphNm7i"),
    },
    StakerInfo {
        name: "alike cheese",
        staker: "Drg9uSvSEfjtn15jqmmrEQnA4pvU1ToYSGSa1Dv9C6Fk",
        lamports: 3_880_295 * LAMPORTS_PER_SOL,
        withdrawer: Some("BxmwgfnyAqZnqRCJGdsEea35pcc92GFTcyGeSj4RNfJJ"),
    },
    StakerInfo {
        name: "noisy honey",
        staker: "95HsPFFvwbWpk42MKzenauSoULNzk8Tg6fc6EiJhLsUZ",
        lamports: 5_000_000 * LAMPORTS_PER_SOL,
        withdrawer: Some("Aj3K933zdRQhYEJi2Yjz8hJWXN3Z3hrKJQtPtE8VmUnq"),
    },
];

pub const SERVICE_STAKER_INFOS: &[StakerInfo] = &[
    StakerInfo {
        name: "wretched texture",
        staker: "B1hegzthtfNQxyEPzkESySxRjMidNqaxrzbQ28GaEwn8",
        lamports: 225_000 * LAMPORTS_PER_SOL,
        withdrawer: Some("HWzeqw1Yk5uiLgT2uGUim5ocFJNCwYUFbeCtDVpx9yUb"),
    },
    StakerInfo {
        name: "unbecoming silver",
        staker: "4AcoZa1P8fF5XK21RJsiuMRZPEScbbWNc75oakRFHiBz",
        lamports: 28_800 * LAMPORTS_PER_SOL,
        withdrawer: None,
    },
    StakerInfo {
        name: "inexpensive uncle",
        staker: "AkJ7yssRqS3X4UWLUsPTxbP6LfVgdPYBWH4Jgk5EETgZ",
        lamports: 300_000 * LAMPORTS_PER_SOL,
        withdrawer: Some("6mudxxoe5VyXXNXsJ3NSGSTGESfG2t86PBCQGbouHpXX"),
    },
    StakerInfo {
        name: "hellish money",
        staker: "CqKdQ57mBj2mKcAbpjWc28Ls7yXzBXboxSTCRWocmUVj",
        lamports: 200_000 * LAMPORTS_PER_SOL,
        withdrawer: None,
    },
    StakerInfo {
        name: "full grape",
        staker: "B2EWnwgmNd3KMpD71yZMijhML1jd4TYp96zJdhMiWZ7b",
        lamports: 450_000 * LAMPORTS_PER_SOL,
        withdrawer: Some("9oaCkokBBhgBsgyg4sL7fMJyQseaJb1TbADZeoPdpWdc"),
    },
    StakerInfo {
        name: "nice ghost",
        staker: "HtQS1CH3nsUHmnLpenj5W6KHzFWTf3mzCn1mTqK7LkB7",
        lamports: 650_000 * LAMPORTS_PER_SOL,
        withdrawer: Some("4YnNnycEZXCkuVs2hDthdNxMD4E8wc7ZPgyAK7Lm1uZc"),
    },
];

pub const FOUNDATION_STAKER_INFOS: &[StakerInfo] = &[
    StakerInfo {
        name: "lyrical supermarket",
        staker: "GRZwoJGisLTszcxtWpeREJ98EGg8pZewhbtcrikoU7b3",
        lamports: 5_000_000 * LAMPORTS_PER_SOL,
        withdrawer: None,
    },
    StakerInfo {
        name: "frequent description",
        staker: "J51tinoLdmEdUR27LUVymrb2LB3xQo1aSHSgmbSGdj58",
        lamports: 57_500_000 * LAMPORTS_PER_SOL,
        withdrawer: None,
    },
];

pub const GRANTS_STAKER_INFOS: &[StakerInfo] = &[
    StakerInfo {
        name: "rightful agreement",
        staker: "DNaKiBwwbbqk1wVoC5AQxWQbuDhvaDVbAtXzsVos9mrc",
        lamports: 5_000_000 * LAMPORTS_PER_SOL,
        withdrawer: None,
    },
    StakerInfo {
        name: "tasty location",
        staker: "HvXQPXAijjG1vnQs6HXVtUUtFVzi5HNgXV9LGnHvYF85",
        lamports: 15_000_000 * LAMPORTS_PER_SOL,
        withdrawer: None,
    },
];

pub const COMMUNITY_STAKER_INFOS: &[StakerInfo] = &[
    StakerInfo {
        name: "shrill charity",
        staker: "Eo1iDtrZZiAkQFA8u431hedChaSUnPbU8MWg849MFvEZ",
        lamports: 5_000_000 * LAMPORTS_PER_SOL,
        withdrawer: Some("8CUUMKYNGxdgYio5CLHRHyzMEhhVRMcqefgE6dLqnVRK"),
    },
    StakerInfo {
        name: "legal gate",
        staker: "7KCzZCbZz6V1U1YXUpBNaqQzQCg2DKo8JsNhKASKtYxe",
        lamports: 16_086_641 * LAMPORTS_PER_SOL,
        withdrawer: Some("92viKFftk1dJjqJwreFqT2qHXxjSUuEE9VyHvTdY1mpY"),
    },
    StakerInfo {
        name: "cluttered complaint",
        staker: "2J8mJU6tWg78DdQVEqMfpN3rMeNbcRT9qGL3yLbmSXYL",
        lamports: 153_333_633 * LAMPORTS_PER_SOL + 41 * LAMPORTS_PER_SOL / 100,
        withdrawer: Some("7kgfDmgbEfypBujqn4tyApjf8H7ZWuaL3F6Ah9vQHzgR"),
    },
];

fn add_stakes(
    genesis_config: &mut GenesisConfig,
    staker_infos: &[StakerInfo],
    unlock_info: &UnlockInfo,
) -> u64 {
    staker_infos
        .iter()
        .map(|staker_info| create_and_add_stakes(genesis_config, staker_info, unlock_info, None))
        .sum::<u64>()
}

pub fn add_genesis_accounts(genesis_config: &mut GenesisConfig, mut issued_lamports: u64) {
    // add_stakes() and add_validators() award tokens for rent exemption and
    //  to cover an initial transfer-free period of the network

    issued_lamports += add_stakes(
        genesis_config,
        &CREATOR_STAKER_INFOS,
        &UNLOCKS_HALF_AT_9_MONTHS,
    ) + add_stakes(
        genesis_config,
        &SERVICE_STAKER_INFOS,
        &UNLOCKS_ALL_AT_9_MONTHS,
    ) + add_stakes(
        genesis_config,
        &FOUNDATION_STAKER_INFOS,
        &UNLOCKS_ALL_DAY_ZERO,
    ) + add_stakes(genesis_config, &GRANTS_STAKER_INFOS, &UNLOCKS_ALL_DAY_ZERO)
        + add_stakes(
            genesis_config,
            &COMMUNITY_STAKER_INFOS,
            &UNLOCKS_ALL_DAY_ZERO,
        );

    // "one thanks" (community pool) gets 500_000_000SOL (total) - above distributions
    create_and_add_stakes(
        genesis_config,
        &StakerInfo {
            name: "one thanks",
            staker: "7vEAL3nS9CWmy1q6njUUyHE7Cf5RmyQpND6CsoHjzPiR",
            lamports: 500_000_000 * LAMPORTS_PER_SOL - issued_lamports,
            withdrawer: Some("3FFaheyqtyAXZSYxDzsr5CVKvJuvZD1WE1VEsBtDbRqB"),
        },
        &UNLOCKS_ALL_DAY_ZERO,
        None,
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
