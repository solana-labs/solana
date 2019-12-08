use crate::{
    stakes::{create_and_add_stakes, StakerInfo},
    unlocks::UnlockInfo,
    validators::{create_and_add_validator, ValidatorInfo},
};
use solana_sdk::{genesis_config::GenesisConfig, native_token::sol_to_lamports};

// 30 month schedule is 1/5th every 6 months for 30 months
const UNLOCKS_BY_FIFTHS_FOR_30_MONTHS: UnlockInfo = UnlockInfo {
    cliff_fraction: 0.2,
    cliff_years: 0.5,
    unlocks: 4,
    unlock_years: 0.5,
    custodian: "11111111111111111111111111111111",
};
// 60 month schedule is 1/10th every 6 months for 60 months
//const UNLOCKS_BY_TENTHS_FOR_60_MONTHS: UnlockInfo = UnlockInfo {
//    cliff_fraction: 0.1,
//    cliff_years: 0.5,
//    unlocks: 9,
//    unlock_years: 0.5,
//    custodian: "11111111111111111111111111111111",
//};

// 60 month schedule is 1/10th every 6 months for 60 months
const UNLOCKS_BY_TENTHS_FOR_60_MONTHS: UnlockInfo = UnlockInfo {
    cliff_fraction: 0.1,
    cliff_years: 0.5,
    unlocks: 9,
    unlock_years: 0.5,
    custodian: "11111111111111111111111111111111",
};

pub const BATCH_FOUR_STAKER_INFOS: &[StakerInfo] = &[
    StakerInfo {
        name: "impossible pizza",
        staker: "CDtJpwRSiPRDGeKrvymWQKM7JY9M3hU7iimEKBDxZyoP",
        sol: 5_000_000.0,
    },
    StakerInfo {
        name: "wretched texture",
        staker: "HbENu65qjWLEB5TrMouSSWLq9mbtGx2bvfhPjk2FpYek",
        sol: 225_000.0,
    },
    StakerInfo {
        name: "nutritious examination",
        staker: "C9CfFpmLDsQsz6wt7MrrZquNB5oS4QkpJkmDAiboVEZZ",
        sol: 5_000_000.0,
    },
    StakerInfo {
        name: "tidy impression",
        staker: "6ne6Rbag4FAnop1KNgVdM1SEHnJEysHSWyqvRpFrzaig",
        sol: 5_000_000.0,
    },
    StakerInfo {
        name: "unbecoming silver",
        // TODO: staker: "42yapY7Vrs5jqht9TCKsPoyb4vDFYcPfRkqAP85NSAQ", WrongSize
        staker: "GS7RFm4nrxzYGcPTmu1otzHzZbURWDKuxo2L4AQDvTg2",
        sol: 28_800.0,
    },
    StakerInfo {
        name: "dramatic treatment",
        staker: "GTyawCMwt3kMb51AgDtfdp97mDot7jNwc8ifuS9qqANg",
        sol: 1_205_602.0,
    },
    StakerInfo {
        name: "angry noise",
        staker: "Fqxs9MhqjKuMq6YwjBG4ktEapuZQ3kj19mpuHLZKtkg9",
        sol: 5_000_000.0,
    },
    StakerInfo {
        name: "hard cousin",
        staker: "9MYDzj7QuAX9QAK7da1GhzPB4gA3qbPNWsW3MMSZobru",
        sol: 5_000_000.0,
    },
    StakerInfo {
        name: "inexpensive uncle",
        staker: "E4DLNkmdL34ejA48ApfPDoFVuD9XWAFqi8bXzBGRhKst",
        sol: 300_000.0,
    },
    StakerInfo {
        name: "lopsided skill",
        staker: "8cV7zCTF5UMrZakZXiL2Jw5uY3ms2Wz4twzFXEY9Kge2",
        sol: 5_000_000.0,
    },
    StakerInfo {
        name: "red snake",
        staker: "JBGnGdLyo7V2z9hz51mnnbyDp9sBACtw5WYH9YRG8n7e",
        sol: 3_655_292.0,
    },
    StakerInfo {
        name: "hellish money",
        staker: "CqKdQ57mBj2mKcAbpjWc28Ls7yXzBXboxSTCRWocmUVj",
        sol: 200_000.0,
    },
    StakerInfo {
        name: "full grape",
        staker: "2SCJKvh7wWo32PtfUZdVZQ84WnMWoUpF4WTm6ZxcCJ15",
        sol: 450_000.0,
    },
    StakerInfo {
        name: "nice ghost",
        staker: "FeumxB3gfzrVQzABBiha8AacKPY3Rf4BTFSh2aZWHqR8",
        sol: 650_000.0,
    },
    StakerInfo {
        name: "jolly year",
        staker: "HBwFWNGPVZgkf3yqUKxuAds5aANGWX62LzUFvZVCWLdJ",
        sol: 5_000_000.0,
    },
    StakerInfo {
        name: "typical initiative",
        staker: "3JMz3kaDUZEVK2JVjRqwERGMp7LbWbgUjAFBb42qxoHb",
        sol: 5_000_000.0,
    },
    StakerInfo {
        name: "deserted window",
        staker: "XTeBBZextvHkoRqDF8yb4hihjcraKQDwTEXhzjd8fip",
        sol: 3_655_292.0,
    },
    StakerInfo {
        name: "eight nation",
        staker: "E5bSU5ywqPiz3ije89ef5gaEC7jy81BAc72Zeb9MqeHY",
        sol: 103_519.0,
    },
    StakerInfo {
        name: "earsplitting meaning",
        staker: "4ZemkSoE75RFE1SVLnnmHcaNWT4qN8KFrKP2wAYfv8CB",
        sol: 5_000_000.0,
    },
    StakerInfo {
        name: "alike cheese",
        staker: "72BGEwYee5txFonmpEarTEKCZVN2UxcSUgdphdhcx3V",
        sol: 3_880_295.0,
    },
    StakerInfo {
        name: "noisy honey",
        staker: "DRp1Scyn4yJZQfMAdQew2x8RtvRmsNELN37JTK5Xvzgn",
        sol: 5_000_000.0,
    },
];

pub const POOL_STAKER_INFOS: &[StakerInfo] = &[
    StakerInfo {
        name: "shrill charity",
        staker: "BzuqQFnu7oNUeok9ZoJezpqu2vZJU7XR1PxVLkk6wwUD",
        sol: 5_000_000.0,
    },
    StakerInfo {
        name: "legal gate",
        staker: "FwMbkDZUb78aiMWhZY4BEroAcqmnrXZV77nwrg71C57d",
        sol: 16_086_641.0,
    },
    StakerInfo {
        name: "cluttered complaint",
        staker: "4h1rt2ic4AXwG7p3Qqhw57EMDD4c3tLYb5J3QstGA2p5",
        sol: 153_333_633.41,
    },
    StakerInfo {
        name: "one thanks",
        staker: "3b7akieYUyCgz3Cwt5sTSErMWjg8NEygD6mbGjhGkduB",
        sol: 178_699_925.59,
    },
    StakerInfo {
        name: "lyrical supermarket",
        staker: "GRZwoJGisLTszcxtWpeREJ98EGg8pZewhbtcrikoU7b3",
        sol: 5_000_000.0,
    },
    StakerInfo {
        name: "frequent description",
        staker: "J51tinoLdmEdUR27LUVymrb2LB3xQo1aSHSgmbSGdj58",
        sol: 57_500_000.0,
    },
    StakerInfo {
        name: "rightful agreement",
        staker: "DNaKiBwwbbqk1wVoC5AQxWQbuDhvaDVbAtXzsVos9mrc",
        sol: 5_000_000.0,
    },
    StakerInfo {
        name: "tasty location",
        staker: "HvXQPXAijjG1vnQs6HXVtUUtFVzi5HNgXV9LGnHvYF85",
        sol: 15_000_000.0,
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
        node_sol: 500.0,
        commission: 0,
    },
    ValidatorInfo {
        name: "Bison Trails",
        node: "7suRNpX7bJsXphHJtBv4ZsLjJZ1dTGeX256pLqJZdEAm",
        vote: "DfirEZ9Up1xbE7sQji9UwtcRGe5uCcRqQtnaGpha5KNY",
        node_sol: 500.0,
        commission: 0,
    },
    ValidatorInfo {
        name: "ChainFlow",
        node: "2te46rxywMdCNdkvjumiBBPQoVczJFxhxEaxFavQNqe3",
        vote: "8bRCnytB7bySmqxodNGbZuUAtncKkB8T733DD1Dm9WMb",
        node_sol: 500.0,
        commission: 0,
    },
    ValidatorInfo {
        name: "ChorusOne",
        node: "ChorusXqjLC2NbiStKR6k9WoD7wu6TVTtFG8qCL5XBVa",
        vote: "ChorusvBuPwukqgDvYfWtEg8j4T1NcMgSTQ4b1UbAwgQ",
        node_sol: 500.0,
        commission: 0,
    },
    ValidatorInfo {
        name: "Dokia Capital",
        node: "GeZ5PrJi9muVCJiJAaFBNGoCEdxGEqTp7L2BmT2WTTy1",
        vote: "7ZdRx2EBYoRuPfyeoNbuHodMUXcAQRcC37MUw3kP6akn",
        node_sol: 500.0,
        commission: 0,
    },
    ValidatorInfo {
        name: "Forbole",
        node: "Fe5sLQAAT7RBT8mcH1AAGCbExJQcYxcwXvp1GjrGbvxs",
        vote: "Dr8MkZZuvZVQJFKtjShZYEfg6n93sc1GxevqLnGss7FW",
        node_sol: 500.0,
        commission: 0,
    },
    ValidatorInfo {
        name: "P2P.ORG - Secure Non-custodial Staking",
        node: "44e8VyWoyZSE2oYHxMHMedAiHkGJqJgPd3tdt6iKoAFL",
        vote: "BwwpzEpo1wzgV9N1987ntgNG6jLt3C9532C68pswT7Gp",
        node_sol: 500.0,
        commission: 0,
    },
    ValidatorInfo {
        name: "RockX",
        node: "Ez4iUU87ViJLCnmSy1t1Ti3DLoysFXiBseNfnRfoehyY",
        vote: "GUdGALCHQBeqkNc2ZAht3tBXab1N5u9qJC3PAzpL54r7",
        node_sol: 500.0,
        commission: 0,
    },
    ValidatorInfo {
        name: "Stake Capital",
        node: "HavuVVDXXsJqMzPwQ4KcF5kFm2xqjbChhyi1bgGeCQif",
        vote: "HswPkKj1xoLLmpM8t1vy5Pbi8zYYUs9ZawswvofKsFo1",
        node_sol: 500.0,
        commission: 0,
    },
    ValidatorInfo {
        name: "Staking Facilities",
        node: "pbAxyqHHPMwgEjv8kmjGxysk9rhNtN7q22eAjReq6Hj",
        vote: "4VZ3pJX19PpuGjoSB1qeN9sVQfrqgLVNg16is37adiFp",
        node_sol: 500.0,
        commission: 0,
    },
];

fn add_validators(genesis_config: &mut GenesisConfig, validator_infos: &[ValidatorInfo]) -> u64 {
    validator_infos
        .iter()
        .map(|validator_info| create_and_add_validator(genesis_config, validator_info))
        .sum::<u64>()
}

fn add_spare_validators(genesis_config: &mut GenesisConfig) -> u64 {
    let node_pubkeys = [
        "id2GQ6YwsjTCCHJ9pJaffC3MEezPscNLjPdGPSfaN46",
        "id2QqCczwAMoDuG4sVZFjooysAqhs6hzMgGigg2rbMV",
        "id4PaxnDQwH6mLjrXZ7S56DoLxKLFdkSBmr6vma6sBf",
        "id7U3WaKEeWgGAyNydEHZFiitkc1bL5VzYmxZXTYfVY",
        "id7ywnFUjQ27BueJJc1U4inAWEvWpMaBX1fXbBKWz2J",
        "idB6NCyjMBTfdMuC9yj8vd8iiajNd9Mbk2AXp8a5xHe",
        "idBpZi4KcCoV5t88BSrnJ98zv9dZkLZQBk54TFyAdaZ",
        "idCfrfPBhvPWxj1N29n7gbMMejPraDsATyWQPFAXJuZ",
        "idEfgYfaLCvtWA7attVbBNgnfokJNpjbXpqLuTSPjUp",
        "idF6btoY9VHbnU5sCYpZ1Bu8yBAL3bKfN2k47ukan3n",
        "idGi3LekrrcVzvnQiydodwM86eqZSo8mWN59ytxZdH5",
        "idGxeLFcK36ZQmaF249uPTZZGnRn7FpXoPJR5LKhv1v",
        "idJ8bnEkJf5CL7FngrM4zm1BHMLT1iMoQnJkL6sbq3B",
        "idKsePUfNbUALy2qiCEh1gFKgjuLc6p6pekmto9R5Cb",
        "idLdNwPPV5Zikk3sVANyXzUquXzQzwmWbib6m15WT8t",
        "idMC8bibeXspJphRNK5HpwK3Lh744fDtksrT2cULH9q",
        "idMi1g3V87WNYa3SXGLzsHeKNFaJVrryF6on5dU7DA2",
        "idNFd41HQWr5NmPrZRwFwxXUnR4YqHpGC3CGke2KBRW",
        "idQwur4HP41cWqDxktp1UeT1PiG6KcTYKEbQeWyQ4BU",
        "idSM4aD8kSQmRxm12yvzaYES3esUeMiFmJdxaPEVYPT",
        "idSMbfe8Up3syM8sgn8Ubzd1FYc77KptdMDbufBv3GS",
        "idSPToD3qXCvFnJiQ5qRHaTQTqh2pFXmXZFZ5XkvYzV",
        "idSn2FMj47RAWVoCb2pgt8YCnfnfwY2vReQMNSPceMA",
        "idVXGKsb7F2nFRW51anXiyHPJTFVmnusnHr5zYJaABA",
        "idVcb8J66gCuAwFjLoKGuuvpxxFnhhfpENhUzR4u7tQ",
        "idVo6gQWhf1qptvnt1YwD6thehshDfzN9GZajC6nXkC",
        "idY8iDUV2VeMqeBCRCHgzNPiASXkJKQhDqWMBqH26c9",
        "idZwgtd3r3MX7kriLRX6q3nuPqtb24ZtthS9L23SR6A",
        "idbdSGgRFPQdrsz7wtc2Tbx7MaL4xA6r98yuxrtqEyS",
        "ideNfZLPpeySDZXUnnwmzhaYTh7DX62i2C52icbaEAv",
        "idgxuQoRvB4nP9jj5HshoYWBTQk13pAisn1d96vEap4",
        "idh5GBgrpiay2jgERL61YhfgurjvxbAG7RzPTimHahQ",
        "idjgVqqz9K7qL3eMGApzuoPsZBpUyjd3EDjBwJZPMmF",
        "idmWehGyjwQKmyw4AKG74srqW32Z8ecMMAzJM4pFami",
        "idrpuShZXt12i7tGrN2gfYQPD7Xvak4Ai23WkqYeSPt",
        "idsdFzMbYy9D2YmFm9QmFMi73FsKN4nV7WV8zMUcMEq",
        "iduRkR7DKVWz34HEPAQXVUhh1tVC9wdwTNXcif4CVdq",
        "iduwx2nrXM6WmHSF7AtxdV4LbZgEdXN866xYdZDx9wB",
        "idxVaEn8v3zGTxqJbevTzjJhHdkA5p68ahcXfBqrka8",
        "idz77S9k24pczEcgB4edV9uQag8ZuzZ3NK3DjxroaLq",
        "idzabijKtknbbsmXVB65wrb4PaMdgGjpHhGJ9psD4d2",
        "ide8fez9zNJBJwaESdAo5xtuk9FuWQRVjUjcaXWnm3D",
    ];

    let vote_pubkeys = [
        "vt1LGG2pV9hDVSKorVcwRnQLSdFepduiNVwynkQUStL",
        "vt24vGJHuoverSX5mA2ibuHuJ2JLZqjQZd8PX11d6UL",
        "vt2W6R7hBSCPEua6YxQ8ocRMkNHtrdsRpYd4cYTonUJ",
        "vt2cVaESGQNDi7bEZ4PcKpyZu8QefgsLZTsDZKAWY1m",
        "vt2v2yrw98Ysimde7pPqsLpfYWMNbdiHGioyF5LvMtR",
        "vt5AnXUoNMTuMXcBf2jFTELDp5RTNNgZB7pwyR5ju74",
        "vtA5iZY8eEkzuwqB899rvoz87fLL1UCqdFQLonpo8Yq",
        "vtAJxXc55YtmZyDT3VZv1gHAMe4B5y4rXstD4pbTtMg",
        "vtBHFdgKyuT9DwGEUyyz6Egczj7Ae7iSAQRaJ3oWVuw",
        "vtFMV55GAPEwyeehSzujxE9SeY6goHiRVExm8rTU3e9",
        "vtGN4RNrcviL3sj13gQSp13227ebGfTxJVLYzFbvVoT",
        "vtGmxDTT5kX9DFDN2ENLAyviZx5tUZShK5PqjZFAcUt",
        "vtJdFE1feE1egNUYYY13XV4DsVPKK3Y16bTSi4QaDMP",
        "vtJpR6DcdmHuTQHqqXzPY9D5M6q7Kayw1Qnyjc2s17i",
        "vtMnzXbg4QvVfDaKS9k3gAWuDzXLa5ihciZgjRjreQ3",
        "vtQeWJmcnBSciJxHQ2G3a79JWi5UYUReYfxx9GsbvzX",
        "vtR9U36uMJwRJnT1pPsQeNZT9KE16M5E4zRz6PzqvFJ",
        "vtTvfuJsYu1VJHVJsh4kyzvu6vSR3fiugxNr9VGFMK9",
        "vtWt9vvYxNt6SLPJaJ2L4jSCyNjSNpyTSZ4HMQpYv5L",
        "vtZLhvVTiXKwdwh5mc5jfh9JSMuFaiQ7gp8VwnbyA9e",
        "vtZS3zch9n4u5ToXJBGdUUkt7iqXJv8yZWmnVkSQrmX",
        "vtaXTjzBJuswneaW6GGizm5n2VmUBeBH2FoVkyjPfBk",
        "vtbhFdJagDUdod27WuAwfduoUvEQLJ3CkqyhyuSuU22",
        "vtdDUSd4gmmgwb35oxeMW2H2ptgEzyPnJt2jqRkNSLU",
        "vtdtwJAqWEaxoHEc593ia2bYvNdXjUFrt6A3qtJLcSj",
        "vte2QxwLx4B8E3ndaExZKaTbLvHpr12xvjxEXWkENcb",
        "vtedxropBTfkRVAeQiMi2bmAmVwKhqq6hUTW3ikZi1t",
        "vtgTbGjVT4EeMfc43K3ANCzjF2HRpXFAeQ8crfxLwMK",
        "vtivkBCoKFe7MsoCoiLharA9dHNg9ZaTH376F5kVE9k",
        "vtjWWWJqBVeuRDFL2z2cwR9rnEK1YUoYzmR9ALEQdz4",
        "vtmmRiGQzQRo1gUFyj7TTyFrX2oRbDe3cYJ4gxmkMAF",
        "vtnA4s2T2qikmL5n1UMxfxjyFJFpqxycZFtVepF4F29",
        "vtnG7CkL5cakMt4wm2eV23rZsWb89bwkVRsiS7Lb3Kt",
        "vtoGcgxrUYWvi8coDRiEeBDMY1TCJMKL3mZnadjKtjC",
        "vtppBusuaWNvEU4KUvNXf6wtJB8YcybxA498Zvq3Vf5",
        "vtqfoB3SuNvJQGGXrnkdLQ6yr6XcAP1weoLX5WaugAb",
        "vtsFGh8cwM1SEF8W4F6a5zxPk9TNbTWuWJrJre2apQ6",
        "vtvdwEKJyCBmAG25mrXKCgTY4PtDHLjpTnngBorGBqj",
        "vtwJMJBWGocQL7ggJ5t8vm1dW3qfHKaYMTRnmRvnK1T",
        "vtwsS4uDrkoRV5Ed91GJ17XPnRLkRW6ysep695u4CiU",
        "vtww7Nu3ChKXs9BU2QLuUJXVK8upvWrq8r1QfgAkMg7",
        "vtyfj9nLUpcneGQhmp5v6Q8Nt7i1cENi6WoFgZEDCwE",
    ];

    node_pubkeys
        .iter()
        .zip(vote_pubkeys.iter())
        .map(|(node, vote)| {
            create_and_add_validator(
                genesis_config,
                &ValidatorInfo {
                    name: "elvis",
                    node,
                    vote,
                    node_sol: 500.0,
                    commission: 0,
                },
            )
        })
        .sum::<u64>()
}

pub fn add_genesis_accounts(genesis_config: &mut GenesisConfig) -> u64 {
    add_stakes(
        genesis_config,
        &BATCH_FOUR_STAKER_INFOS,
        &UNLOCKS_BY_FIFTHS_FOR_30_MONTHS,
        sol_to_lamports(1_000_000.0),
    ) + add_stakes(
        genesis_config,
        &POOL_STAKER_INFOS,
        &UNLOCKS_BY_TENTHS_FOR_60_MONTHS,
        sol_to_lamports(1_000_000.0),
    ) + add_validators(genesis_config, &VALIDATOR_INFOS)
        + add_spare_validators(genesis_config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_genesis_accounts() {
        let mut genesis_config = GenesisConfig::default();

        let bootstrap_lamports = genesis_config
            .accounts
            .iter()
            .map(|(_, account)| account.lamports)
            .sum::<u64>();

        let issued_lamports = add_genesis_accounts(&mut genesis_config);

        let lamports = genesis_config
            .accounts
            .iter()
            .map(|(_, account)| account.lamports)
            .sum::<u64>();

        assert_eq!(issued_lamports, lamports);
        let num_spare_validators = 42;
        let rent_fees = 2 * (VALIDATOR_INFOS.len() + num_spare_validators) as u64; // TODO: Need a place to pay rent from.
        let expected_lamports = 500_000_000_000_000_000 - bootstrap_lamports + rent_fees;
        assert_eq!(lamports, expected_lamports);
    }
}
