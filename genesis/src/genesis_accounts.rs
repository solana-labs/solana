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

// 1st batch
const BATCH_ONE_STAKER_INFOS: &[StakerInfo] = &[
    StakerInfo {
        name: "diligent bridge",
        staker: "BwwM47pLHwUgjJXKQKVNiRfGhtPNWfNLH27na2HJQHhd",
        sol: 6_250_000.0,
    },
    StakerInfo {
        name: "four wish",
        staker: "8A6ZEEW2odkqXNjTWHNG6tUk7uj6zCzHueTyEr9pM1tH",
        sol: 10_000_000.0,
    },
    StakerInfo {
        name: "simple friends",
        staker: "D89HyaBmr2WmrTehsfkQrY23wCXcDfsFnN9gMfUXHaDd",
        sol: 1_250_000.0,
    },
    StakerInfo {
        name: "noxious leather",
        staker: "FwPvDpvUmnco1CSfwXQDTbUbuhG5eP7h2vgCKYKVL7at",
        sol: 6_250_000.0,
    },
    StakerInfo {
        name: "worthless direction",
        staker: "4K16iBoC9kAQRT8pUEKeD2h9WEx1zsRgEmJFssXcXmqq",
        sol: 12_500_000.0,
    },
    StakerInfo {
        name: "historical company",
        staker: "rmLpENW4V6QNeEhdJJVxo9Xt99oKgNUFZS4Y4375amW",
        sol: 322_850.0,
    },
    StakerInfo {
        name: "callous money",
        staker: "5kAztE3XtrpeyGZZxckSUt3ZWojNTmph1QSC9S2682z4",
        sol: 5_927_155.25,
    },
    StakerInfo {
        name: "outstanding jump",
        staker: "H6HMVuDR8XCw3EuhLvFG4EciVvGo76Agq1kSBL2ozoDs",
        sol: 625_000.0,
    },
    StakerInfo {
        name: "feeble toes",
        staker: "3sfv8tk5ZSDBWbTkFkvFxCvJUyW5yDJUu6VMJcUARQWq",
        sol: 750_000.0,
    },
    StakerInfo {
        name: "disillusioned deer",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 1_250_000.0,
    },
    StakerInfo {
        name: "unwritten songs",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 4_250_000.0,
    },
    StakerInfo {
        name: "overt dime",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 500_000.0,
    },
    StakerInfo {
        name: "slow committee",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 625_000.0,
    },
    StakerInfo {
        name: "curvy twig",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 625_000.0,
    },
    StakerInfo {
        name: "gamy scissors",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 250_000.0,
    },
    StakerInfo {
        name: "mushy key",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 1_250_000.0,
    },
    StakerInfo {
        name: "marked silver",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 250_000.0,
    },
    StakerInfo {
        name: "free sock",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 625_000.0,
    },
    StakerInfo {
        name: "tremendous meeting",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 1_250_000.0,
    },
    StakerInfo {
        name: "panoramic cloth",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 625_000.0,
    },
    StakerInfo {
        name: "normal kick",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 2_500_000.0,
    },
    StakerInfo {
        name: "unbecoming observation",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 250_000.0,
    },
    StakerInfo {
        name: "cut beginner",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 250_000.0,
    },
    StakerInfo {
        name: "alcoholic button",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 625_000.0,
    },
    StakerInfo {
        name: "old-fashioned clover",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 750_000.0,
    },
    StakerInfo {
        name: "expensive underwear",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 2_500_000.0,
    },
    StakerInfo {
        name: "like dust",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 5_000_000.0,
    },
    StakerInfo {
        name: "rapid straw",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 5_850_000.0,
    },
    StakerInfo {
        name: "windy trousers",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 2_579_350.0,
    },
    StakerInfo {
        name: "dramatic veil",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 3_611_110.50,
    },
    StakerInfo {
        name: "incandescent skin",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 3_000_000.0,
    },
    StakerInfo {
        name: "spiky love",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 3_250_000.0,
    },
];

const BATCH_TWO_STAKER_INFOS: &[StakerInfo] = &[
    // 2nd batch
    StakerInfo {
        name: "macabre note",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 4_000_000.0,
    },
    StakerInfo {
        name: "alcoholic letter",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 4_000_000.0,
    },
    StakerInfo {
        name: "heady trucks",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 4_000_000.0,
    },
    StakerInfo {
        name: "ten support",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 1_000_000.0,
    },
    StakerInfo {
        name: "foregoing middle",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 800_000.0,
    },
    StakerInfo {
        name: "ludicrous destruction",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 4_000_000.0,
    },
    StakerInfo {
        name: "numberless wheel",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 4_000_000.0,
    },
    StakerInfo {
        name: "short powder",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 4_000_000.0,
    },
    StakerInfo {
        name: "cut name",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 4_000_000.0,
    },
    StakerInfo {
        name: "six fly",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 4_000_000.0,
    },
    StakerInfo {
        name: "mindless pickle",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 100_000.0,
    },
    StakerInfo {
        name: "marked rabbit",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 38_741.36,
    },
    StakerInfo {
        name: "jagged doctor",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 711_258.64,
    },
    StakerInfo {
        name: "truthful pollution",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 1_587_300.0,
    },
    StakerInfo {
        name: "unkempt activity",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 2_222_220.0,
    },
    StakerInfo {
        name: "ritzy view",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 40_000.0,
    },
    StakerInfo {
        name: "remarkable plant",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 300_000.0,
    },
    StakerInfo {
        name: "busy value",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 100_000.0,
    },
    StakerInfo {
        name: "imperfect slave",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 222_065.84,
    },
    StakerInfo {
        name: "uneven drawer",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 400_000.0,
    },
    StakerInfo {
        name: "far behavior",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 4_000_000.0,
    },
    StakerInfo {
        name: "abaft memory",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 400_000.0,
    },
    StakerInfo {
        name: "poor glove",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 2_000_000.0,
    },
    StakerInfo {
        name: "strange iron",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 2_000_000.0,
    },
    StakerInfo {
        name: "nonstop rail",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 1_000_000.0,
    },
    StakerInfo {
        name: "milky bait",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 400_000.0,
    },
    StakerInfo {
        name: "wandering start",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 1_200_000.0,
    },
];

pub const BATCH_THREE_STAKER_INFOS: &[StakerInfo] = &[
    // 3rd batch
    StakerInfo {
        name: "dusty dress",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 1_212_121.21,
    },
    StakerInfo {
        name: "godly bed",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 151_515.15,
    },
    StakerInfo {
        name: "innocent property",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 227_272.73,
    },
    StakerInfo {
        name: "responsible bikes",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 3_030_303.03,
    },
    StakerInfo {
        name: "learned market",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 3_030_303.03,
    },
    StakerInfo {
        name: "jumpy school",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 303_030.30,
    },
    StakerInfo {
        name: "sticky houses",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 1_515_151.52,
    },
    StakerInfo {
        name: "bustling basketball",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 1_515_152.52,
    },
    StakerInfo {
        name: "ordinary dad",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 606_060.61,
    },
    StakerInfo {
        name: "absurd bat",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 90_909.09,
    },
    StakerInfo {
        name: "cloudy ocean",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 67_945.45,
    },
    StakerInfo {
        name: "black-and-white fold",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 757_575.76,
    },
    StakerInfo {
        name: "stale part",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 45_454.55,
    },
    StakerInfo {
        name: "available health",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 2_797_575.76,
    },
    StakerInfo {
        name: "afraid visitor",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 481_818.18,
    },
    StakerInfo {
        name: "arrogant front",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 151_515.15,
    },
    StakerInfo {
        name: "juvenile zinc",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 151_515.15,
    },
    StakerInfo {
        name: "disturbed box",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 303_030.30,
    },
    StakerInfo {
        name: "disagreeable skate",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 454_545.45,
    },
    StakerInfo {
        name: "miscreant sidewalk",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 75_757.58,
    },
    StakerInfo {
        name: "shy play",
        staker: "P1aceHo1derPubkey11111111111111111111111111",
        sol: 303_030.30,
    },
];

pub const BATCH_FOUR_STAKER_INFOS: &[StakerInfo] = &[
    StakerInfo {
        name: "deserted window",
        staker: "XTeBBZextvHkoRqDF8yb4hihjcraKQDwTEXhzjd8fip",
        sol: 3_655_292.0,
    },
    StakerInfo {
        name: "hard cousin",
        staker: "9MYDzj7QuAX9QAK7da1GhzPB4gA3qbPNWsW3MMSZobru",
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
        sol: 5_000_000.0,
    },
    StakerInfo {
        name: "cluttered complaint",
        staker: "4h1rt2ic4AXwG7p3Qqhw57EMDD4c3tLYb5J3QstGA2p5",
        sol: 5_000_000.0,
    },
    StakerInfo {
        name: "one thanks",
        staker: "3b7akieYUyCgz3Cwt5sTSErMWjg8NEygD6mbGjhGkduB",
        sol: 5_000_000.0,
    },
    StakerInfo {
        name: "on buzzer",
        staker: "GRZwoJGisLTszcxtWpeREJ98EGg8pZewhbtcrikoU7b3",
        sol: 5_000_000.0,
    },
    StakerInfo {
        name: "last minute",
        staker: "J51tinoLdmEdUR27LUVymrb2LB3xQo1aSHSgmbSGdj58",
        sol: 5_000_000.0,
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
        name: "P2P.ORG - Secure Non-custodial Staking",
        node: "44e8VyWoyZSE2oYHxMHMedAiHkGJqJgPd3tdt6iKoAFL",
        vote: "BwwpzEpo1wzgV9N1987ntgNG6jLt3C9532C68pswT7Gp",
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
        name: "RockX",
        node: "Ez4iUU87ViJLCnmSy1t1Ti3DLoysFXiBseNfnRfoehyY",
        vote: "GUdGALCHQBeqkNc2ZAht3tBXab1N5u9qJC3PAzpL54r7",
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
        &BATCH_ONE_STAKER_INFOS,
        &UNLOCKS_BY_FIFTHS_FOR_30_MONTHS,
        sol_to_lamports(1_000_000.0),
    ) + add_stakes(
        genesis_config,
        &BATCH_TWO_STAKER_INFOS,
        &UNLOCKS_BY_FIFTHS_FOR_30_MONTHS,
        sol_to_lamports(1_000_000.0),
    ) + add_stakes(
        genesis_config,
        &BATCH_THREE_STAKER_INFOS,
        &UNLOCKS_BY_FIFTHS_FOR_30_MONTHS,
        sol_to_lamports(1_000_000.0),
    ) + add_stakes(
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

        let issued_lamports = add_genesis_accounts(&mut genesis_config);

        let lamports = genesis_config
            .accounts
            .iter()
            .map(|(_, account)| account.lamports)
            .sum::<u64>();

        assert_eq!(issued_lamports, lamports);
    }
}
