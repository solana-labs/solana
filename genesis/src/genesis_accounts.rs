use crate::{
    stakes::{create_and_add_stakes, StakerInfo},
    unlocks::UnlockInfo,
    validators::create_and_add_validator,
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

pub const VALIDATOR_PUBKEYS: &[&str] = &[
    "27SB7d27xvtBJjgsAV8JBDjQroySmZepiNSepeRbRhe9", //
    "2te46rxywMdCNdkvjumiBBPQoVczJFxhxEaxFavQNqe3", // ChainFlow
    "2tvTYUajoBgeAd66Zhd5Nc2BgKGCgdmasR94fpBokzop", //
    "3QZDKya4AHzsLAuRaMRgejrqW6mETnX88aMSkm7FEE7E", // Syncnode SRL
    "3Z5XVczCTXeYeFABoeFm1LngC9657kZMVGNFzqFXviHb", // stake.fish
    "3o43fXxTpndqVsCdMi16WNq6aR9er75P364ZkTHrEQJN", // mintonium
    "44e8VyWoyZSE2oYHxMHMedAiHkGJqJgPd3tdt6iKoAFL", // P2P.ORG - Secure Non-custodial Staking
    "4MHRFcPheQonBf1pUKrBwJAnn2wP9NEZkXYFEFMfFbWV", //
    "4vPqTnfH2ud6hp1yFSFRy9t9xhm8sGDwU4amcZGr2gT7", // Certus One / nexantic GmbH
    "4ydifDThiWuVtzV92eNGiznuQAnTZtGkb9b2XQoMGAUn", // Sikka
    "54g6LdVubwthdfMKwPqLraDEDAVWNDpN6a3ZGZm2Sbjz", //
    "592eDka2qrXWcszC3NNvViKfEyxvuoAbBgohVt75dWq1", //
    "5JuyDi5HR2CZS39nF43Ws6nhqYWM2fgnZbtf9zRNy52a", // stakewolf
    "5jTcJaq6gLEao1R5rscvfnUhNt6RXg4JFDCegyEhsJG2", //
    "5n8KCdzqtvTnhkvCrFR7errH6ZUp11kL97r2awXkfzFe", // 01Node
    "7ntcPwcaCSpH66ftVZU5oSuWSpvQfN3kfTDaGUHWsc1m", // IZO DATA NETWORK
    "7sa8uUnjNPJ2dFwrKG2kd1XEiB4ujsJ4wGEWn7CK629K", // SparkPool
    "7suRNpX7bJsXphHJtBv4ZsLjJZ1dTGeX256pLqJZdEAm", // Bison Trails
    "7v5DXDvYzkgTdFYXYB12ZLKD6z8QfzR53N9hg6XgEQJE", // Cryptium Labs GmbH
    "8LSwP5qYbmuUfKLGwi8XaKJnai9HyZAJTnBovyWebRfd", //
    "8UPb8LMWyoJJC9Aeq9QmTzKZKV2ssov739bTJ14M4ws1", //
    "8wFK4fCAuDoAH1fsgou9yKZPqDMFtJUVoDdkZAAMuhyA", // LunaNova Technologies Ltd
    "94eWgQm2k8BXKEWbJP2eScHZeKopXpqkuoVrCofQWBhW", // Node A-Team
    "9J8WcnXxo3ArgEwktfk9tsrf4Rp8h5uPUgnQbQHLvtkd", // moonli.me
    "AYZS4CFZRi1165mmUqnpDcHbm1NT9zFGPdjG5VDuK79p", // Ubik Capital
    "Ah5arzkbkHTMkzUaD5DiCAC1rzxqPgyQDFTnw8Krwz1V", // Moonlet
    "ArpeD4LKYgza1o6aR5xNTQX3hxeik8URxWNQVpA8wirV", // Staking Fund
    "B21L2hCrdE4SDhRi2fHKohfSUNAhoLeaWfBp1C9HdF6Y", // Everstake
    "Bf6JtoLAg9zxAksgZ9gUsa6zZum1UuPWuirY6qKLXXoW", //
    "BrFqUxNY4HstYdiYYZiyDa5KiTrdcfqoBBEky3kqKFgQ", // IRIS Foundation Ltd.
    "C8VJytJbZM7KFMXHNUdoF4V7V2QbhkxNs1qYybRoqUEK", //
    "CWfPaZJpy8fc2eU7qe1JNnf4oszQFJU68DZiVJGGy4Z7", //
    "Ccq6zHdtv3DWCP4AccTi4Ya2xPGsEVHSfoPmQ1qffb8H", //
    "ChorusXqjLC2NbiStKR6k9WoD7wu6TVTtFG8qCL5XBVa", // ChorusOne
    "DaqUBvjHtKYiZ6exUhqrcpDqu5ffYB6QWKwXSwdvDVBj", //
    "Daxixc1dFxxLDj85t1CWAsvNXdYq51tDAE51nhPqK9yF", //
    "Dh1DRj5mLYMeJVGvaPZN7F4XjpX6u2dCDXVnUXrE8rwW", // POS Bakerz
    "DxLtXrLUrqja3EFjkR4PXNYCuyVtaQnozonCdf3iZk8X", // melea
    "ETVHRnFkZi7PihPDYibp9fmjfR8P5o7pEs92czku62VV", //
    "EduAgutprA7Vp94ZmTU6WRAmqq7VZAXBqH1GyxjWn12D", //
    "Ez4iUU87ViJLCnmSy1t1Ti3DLoysFXiBseNfnRfoehyY", // RockX
    "FYbyeGqsx8G5mW4p3MfnNEsHaCQQSAmxESf7ct36moGZ", //
    "Fe5sLQAAT7RBT8mcH1AAGCbExJQcYxcwXvp1GjrGbvxs", // Forbole Limited
    "FhacRVSACfKcZNAbvbKuj1MunBKxQu2nHu9raJaGsZzG", //
    "FiF184p8DYxnWkBc7WxUh49PccYwvVepmk3nxAnNgGqW", // Easy 2 Stake
    "G47WACh32JUcxyiCna7UYw45tyYSFKQ58yFpUmhmMybm", //
    "GRi3H2M3HxYGAKhz5VrUQipUrAhWj6jTbtjhxiKXHhRj", //
    "GeZ5PrJi9muVCJiJAaFBNGoCEdxGEqTp7L2BmT2WTTy1", // Dokia Capital
    "GkNQ9hQM1DoTQy9i4HVzhCjtKh9A6uSx7Z5XTAkqRGhu", //
    "GsEofbB3rzUK78Ee4NRL6AmcPs6FKRCb7JA8tX6LZjHc", //
    "H279DmgqTkTYnEucPdKbvT8wMTGBAuVh787FX2gRT5Bg", //
    "Hac7hGYwbve747fGefaFoank1K1rNmvr5MjtsYvzZ37i", //
    "HavuVVDXXsJqMzPwQ4KcF5kFm2xqjbChhyi1bgGeCQif", // Stake Capital
    "HpzcHxARoR6HtVuZPXWJMLwgusk2UNCan343u6WSQvm2", //
    "Luna1VCsPBE4hghuHaL9UFgimBB3V6u6johyd7hGXBL",  // LunaNova Technologies Ltd
    "SPC3m89qwxGbqYdg1GuaoeZtgJD2hYoob6c4aKLG1zu",  // Staker Space
    "Smith4JYx2otuFgT2dR83qJSfW8RjBZHPsXPyfZBYBu",  // MCF
    "pbAxyqHHPMwgEjv8kmjGxysk9rhNtN7q22eAjReq6Hj",  // Staking Facilities
    "qzCAEHbjb7AtpTPKaetY47LWNLCxFeHuFozjeXhj1k1",  // Figment Networks, Inc.
];

fn add_validators(genesis_config: &mut GenesisConfig, validator_pubkeys: &[&str]) -> u64 {
    validator_pubkeys
        .iter()
        .map(|validator_pubkey| {
            create_and_add_validator(genesis_config, validator_pubkey, 500 * LAMPORTS_PER_SOL)
        })
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
    ) + add_validators(genesis_config, &VALIDATOR_PUBKEYS);

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

    #[test]
    fn test_no_duplicate_validator_pubkeys() {
        let mut v = VALIDATOR_PUBKEYS.to_vec();
        v.sort();
        v.dedup();
        assert_eq!(v.len(), VALIDATOR_PUBKEYS.len());
    }
}
