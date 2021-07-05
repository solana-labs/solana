use crate::{
    stakes::{create_and_add_stakes, StakerInfo},
    unlocks::UnlockInfo,
};
use solana_sdk::{genesis_config::GenesisConfig, native_token::LAMPORTS_PER_SAFE};

// 9 month schedule is 100% after 9 months
const UNLOCKS_ALL_AT_9_MONTHS: UnlockInfo = UnlockInfo {
    cliff_fraction: 1.0,
    cliff_years: 0.75,
    unlocks: 0,
    unlock_years: 0.0,
    custodian: "4jAMsnkjcRapWdVaXMvJc1QMcD53t1tbqnwXQmdRWGRe",
};

// 9 month schedule is 50% after 9 months, then monthly for 2 years
const UNLOCKS_HALF_AT_9_MONTHS: UnlockInfo = UnlockInfo {
    cliff_fraction: 0.5,
    cliff_years: 0.75,
    unlocks: 24,
    unlock_years: 2.0,
    custodian: "4jAMsnkjcRapWdVaXMvJc1QMcD53t1tbqnwXQmdRWGRe",
};

// no lockups
const UNLOCKS_ALL_DAY_ZERO: UnlockInfo = UnlockInfo {
    cliff_fraction: 1.0,
    cliff_years: 0.0,
    unlocks: 0,
    unlock_years: 0.0,
    custodian: "4jAMsnkjcRapWdVaXMvJc1QMcD53t1tbqnwXQmdRWGRe",
};

pub const CREATOR_STAKER_INFOS: &[StakerInfo] = &[
    StakerInfo {
        name: "impossible pizza",
        staker: "H1fNXtpCjM1DgivVDZi6hPfBfGUhjr2ge39kMoQuzqj9",
        lamports: 5_000_000 * LAMPORTS_PER_SAFE,
        withdrawer: Some("GRVQKDwGXiF4zHxneA3UFUWM2RvxJawQnrVfUeFVZcWk"),
    },
    StakerInfo {
        name: "nutritious examination",
        staker: "HpbLHxUPPJHNphsZbemwrM7VccAMa7rCSCCfZGeEYo2L",
        lamports: 5_000_000 * LAMPORTS_PER_SAFE,
        withdrawer: Some("AatbT6jw9CMTP4fmxdH4pTyPMo7xsrCfLbDJTfTasxSq"),
    },
    StakerInfo {
        name: "tidy impression",
        staker: "DDt2fQmEXRyUc7mbcg52LsYZfR9Rywf6cD3dgTQXbypF",
        lamports: 5_000_000 * LAMPORTS_PER_SAFE,
        withdrawer: Some("DYSREp8n7cfSTULpP1j1n6m6sXpWmayPt51SLqo9u3im"),
    },
    StakerInfo {
        name: "dramatic treatment",
        staker: "4Tyr3rfqkLtLabRB73g2oh8pyNiKJE4mVpN7MAXBAaxp",
        lamports: 1_205_602 * LAMPORTS_PER_SAFE,
        withdrawer: Some("5j7rZgBG25EfrYd2yAChvm7kxYuDrxukPuq2mVRQWXUT"),
    },
    StakerInfo {
        name: "angry noise",
        staker: "98KKVccS35MYw8htHRdnPk5RrFBjsgRcxLCwKsZdGhxH",
        lamports: 5_000_000 * LAMPORTS_PER_SAFE,
        withdrawer: Some("51JxsWkEdqGSMmHXZcB5XwST74x1phg6nd2YJKC7mEZy"),
    },
    StakerInfo {
        name: "hard cousin",
        staker: "9oaPR4mCSBCvJKTKLepMK4LgyPD96ndn91ppfwMiSQid",
        lamports: 5_000_000 * LAMPORTS_PER_SAFE,
        withdrawer: Some("4psr4eV1qCB39cNntksBgCDUBwMGvYNTz56wDQVYDfCT"),
    },
    StakerInfo {
        name: "lopsided skill",
        staker: "2kdBFH22Bn5fhfEH89mBNef9gesvhyeskYkjGjcguHU3",
        lamports: 5_000_000 * LAMPORTS_PER_SAFE,
        withdrawer: Some("Ahra6gCHcPYqifGkPPaG4zyxH8Pz8yLL1FWHvvynHGsY"),
    },
    StakerInfo {
        name: "red snake",
        staker: "5AHbkJTgvM5JBhSSALfKao53qnYuXCD9xgojM6CD1uBa",
        lamports: 3_655_292 * LAMPORTS_PER_SAFE,
        withdrawer: Some("NxYKW7jdw4J8dcgFy4kcJifhR2NNQYGuZMWYqXSt2ei"),
    },
    StakerInfo {
        name: "jolly year",
        staker: "5oSe8xAjNzqaSEPu6pDBdv7zz7RpRLuUsN6ymwY84hb8",
        lamports: 5_000_000 * LAMPORTS_PER_SAFE,
        withdrawer: Some("CtgprLJGpwVYgRZEmpWruiJfpZBDmdEVRmyEVHqD5qys"),
    },
    StakerInfo {
        name: "typical initiative",
        staker: "An3J1VzZE9Eh9XdRPVjCsLVQxTwtbvsgajyYHDnT7ofA",
        lamports: 5_000_000 * LAMPORTS_PER_SAFE,
        withdrawer: Some("ARczCpxsEW7wsXgYx88FNNcwoNCHnthzMvzKgpVJDtCH"),
    },
    StakerInfo {
        name: "deserted window",
        staker: "DoiL85KSqLaNFQnTrAJjmNENp4TsKMSnsNnKirCscBo2",
        lamports: 3_655_292 * LAMPORTS_PER_SAFE,
        withdrawer: Some("2QQGCqQhkrJc4V4d2beAXkEFdsiDocWRqAVZ5EF2Vv5a"),
    },
    StakerInfo {
        name: "eight nation",
        staker: "F3S5Z68M86wCvM1CskRhhUxn5QDMPpeZSZnwDihYUieo",
        lamports: 103_519 * LAMPORTS_PER_SAFE,
        withdrawer: Some("GQSMPe9ax6NgYDux8yjJRrgbACFiAoCxGRAFcGeGZmzH"),
    },
    StakerInfo {
        name: "earsplitting meaning",
        staker: "HnYNmSjgixZjoT6HkQ4xLuKwBE2MWmDXARck4eeKSL13",
        lamports: 5_000_000 * LAMPORTS_PER_SAFE,
        withdrawer: Some("9dkJUJPsGnrxK3ukC6QmSgTNWx1YbVocCfeXiJH4gG3H"),
    },
    StakerInfo {
        name: "alike cheese",
        staker: "E9PF3TxSM4L2k5c9D99NXohdLhze5qiKez4ALc4G2Ju5",
        lamports: 3_880_295 * LAMPORTS_PER_SAFE,
        withdrawer: Some("3qiaDaNPTKK6YJCR8AXAHmeyprG6xtiCWh7CSzCcnc8Q"),
    },
    StakerInfo {
        name: "noisy honey",
        staker: "9qgPPurxWyv3ceWVLAdRDEa3UVXQZWF7YKj7yfT4PSUi",
        lamports: 5_000_000 * LAMPORTS_PER_SAFE,
        withdrawer: Some("3zu3Zk9o5wkc2bxjQiN3uz4ve1aeBysHeEqUWZqNkrgt"),
    },
];

pub const SERVICE_STAKER_INFOS: &[StakerInfo] = &[
    StakerInfo {
        name: "wretched texture",
        staker: "8GFZFg4pn7HBXWaAYr8B4z2VQunmmv3NLjwoBbVjXtJF",
        lamports: 225_000 * LAMPORTS_PER_SAFE,
        withdrawer: Some("Hu5faNrsFGzUXPVztzdt4qthBJgfCV7UBfi2LxFZAQQF"),
    },
    StakerInfo {
        name: "unbecoming silver",
        staker: "BfdVgQUHoFqqUSLGPp9znubV97tZh2kFZXUXtEfkMLsi",
        lamports: 28_800 * LAMPORTS_PER_SAFE,
        withdrawer: None,
    },
    StakerInfo {
        name: "inexpensive uncle",
        staker: "EP5ajhZ5r5EXeFjqFeyZGp8hVbxdYY3bQXeNoYnCwcXU",
        lamports: 300_000 * LAMPORTS_PER_SAFE,
        withdrawer: Some("7NtsARSUFzPn4o61KUy8giJkxYUePwyo6LQFrE4UmH2s"),
    },
    StakerInfo {
        name: "hellish money",
        staker: "6Ra2cHbsnmnfdKywMdEYixLJNLZ6GZbeXKg9TtpcyQ21",
        lamports: 200_000 * LAMPORTS_PER_SAFE,
        withdrawer: Some("3KcTcESyeiZ7sMuGdw6d3Es4NQazgWJVYSbvLU2EiKn2"),
    },
    StakerInfo {
        name: "full grape",
        staker: "J7ZB8Hei5rPPM8yYwYfTNdSzLoBQvJ17tU2fgrZFHBwR",
        lamports: 450_000 * LAMPORTS_PER_SAFE,
        withdrawer: Some("5GTPftxKzYEnc7gfTAQEM1JP4fUrATgrAwBhPPKrVp6z"),
    },
    StakerInfo {
        name: "nice ghost",
        staker: "5czAaD7DBHsK7uPKDdf8ywJ7yiLPWBGvqowW65NZhK1c",
        lamports: 650_000 * LAMPORTS_PER_SAFE,
        withdrawer: Some("13eZuabXGabLPxXYFWAmKYqKjv3TwWrpyxTYuaTCCWGi"),
    },
];

pub const FOUNDATION_STAKER_INFOS: &[StakerInfo] = &[
    StakerInfo {
        name: "lyrical supermarket",
        staker: "ALbso48GLoZDHhJcSXT8PWRHzUonawge3oqrYAf7HoEL",
        lamports: 5_000_000 * LAMPORTS_PER_SAFE,
        withdrawer: Some("X282278JqGZYmRd9NWSkWD8KGK5Ap2D2ZZ3HsDV6X1R"),
    },
    StakerInfo {
        name: "frequent description",
        staker: "342HTFqt57H6U6pDT5j5FadfVFHTCXfBhrZubgwxRzzq",
        lamports: 57_500_000 * LAMPORTS_PER_SAFE,
        withdrawer: Some("6DB5E4mEyFzr9n6jt2aew73cjfysiHegLbmHmC4xME3u"),
    },
];

pub const GRANTS_STAKER_INFOS: &[StakerInfo] = &[
    StakerInfo {
        name: "rightful agreement",
        staker: "HjhARBGyctUWBAnykjy5saxCUFZMuppEZZJgmBXozRnR",
        lamports: 5_000_000 * LAMPORTS_PER_SAFE,
        withdrawer: Some("ErTjf4ggWKHSAyfLkXv7CdkyU6LDJREPZc8437sD6XCc"),
    },
    StakerInfo {
        name: "tasty location",
        staker: "CHq4Bzc9AUURsYiCnufFNPgScUpQMNULPD3x9a3UGkDb",
        lamports: 15_000_000 * LAMPORTS_PER_SAFE,
        withdrawer: Some("FJjDgextVPivPgsFz6WNd22ucFtM9F7RBmY5Q1LbMztZ"),
    },
];

pub const COMMUNITY_STAKER_INFOS: &[StakerInfo] = &[
    StakerInfo {
        name: "shrill charity",
        staker: "E2cRWP3yGyatJoNk1DPapuuHzFr7SricQCDuU9RZmiaE",
        lamports: 5_000_000 * LAMPORTS_PER_SAFE,
        withdrawer: Some("ETb9UBuunEPA1RrwwZ9WrkJ4BJ1836ZUcE9UfRYnDQRK"),
    },
    StakerInfo {
        name: "legal gate",
        staker: "5e3L2HkA8Xt39CWLzdkCR5FGysLihvSZ3ThTs73uepEG",
        lamports: 30_301_032 * LAMPORTS_PER_SAFE,
        withdrawer: Some("4ub1gq72mtHrqc3p4J8pVMfCLjhc6eCp1DHyu4A8b55r"),
    },
    StakerInfo {
        name: "cluttered complaint",
        staker: "7R5XpQeXubDfoMLEmpeHcqFp8jabnuFFtGs26NZ9Gfwn",
        lamports: 153_333_633 * LAMPORTS_PER_SAFE + 41 * LAMPORTS_PER_SAFE / 100,
        withdrawer: Some("CUCCT2eijRtQHkmX4Gtzvp8sGsFkKFHiN5LBd1xLrwyL"),
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

    // "one thanks" (community pool) gets 33_370_166SAFE (total) - above distributions
    create_and_add_stakes(
        genesis_config,
        &StakerInfo {
            name: "one thanks",
            staker: "46PJciJDDeYjTT4ycj19KedEihEe524nWzdWUKj1QgHf",
            lamports: (33_370_166 * LAMPORTS_PER_SAFE).saturating_sub(issued_lamports),
            withdrawer: Some("3DSuNXv7qzvcB6TKAxzaREvW85vit7xw4m2NBWf75585"),
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

        assert_eq!(33_370_166 * LAMPORTS_PER_SAFE, lamports);
    }
}
