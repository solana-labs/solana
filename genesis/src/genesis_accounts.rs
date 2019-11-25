use crate::{
    stakes::{create_and_add_stakes, StakerInfo},
    unlocks::UnlockInfo,
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
        staker: "ab22196afde08a090a3721eb20e3e1ea84d36e14d1a3f0815b236b300d9d33ef",
        withdrawer: "a2a7ae9098f862f4b3ba7d102d174de5e84a560444c39c035f3eeecce442eadc",
        sol: 6_250_000.0,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "four wish",
        staker: "6a56514c29f6b1de4d46164621d6bd25b337a711f569f9283c1143c7e8fb546e",
        withdrawer: "b420af728f58d9f269d6e07fbbaecf6ed6535e5348538e3f39f2710351f2b940",
        sol: 10_000_000.0,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "simple friends",
        staker: "ddf2e4c81eafae2d68ac99171b066c87bddb168d6b7c07333cd951f36640163d",
        withdrawer: "312fa06ccf1b671b26404a34136161ed2aba9e66f248441b4fddb5c592fde560",
        sol: 1_250_000.0,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "noxious leather",
        staker: "0cbf98cd35ceff84ca72b752c32cc3eeee4f765ca1bef1140927ebf5c6e74339",
        withdrawer: "467e06fa25a9e06824eedc926ce431947ed99c728bed36be54561354c1330959",
        sol: 6_250_000.0,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "worthless direction",
        staker: "ef1562bf9edfd0f5e62530cce4244e8de544a3a30075a2cd5c9074edfbcbe78a",
        withdrawer: "2ab26abb9d8131a30a4a63446125cf961ece4b926c31cce0eb84da4eac3f836e",
        sol: 12_500_000.0,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "historical company",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 322_850.0,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "callous money",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 5_927_155.25,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "outstanding jump",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 625_000.0,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "feeble toes",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 750_000.0,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "disillusioned deer",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 1_250_000.0,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "unwritten songs",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 4_250_000.0,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "overt dime",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 500_000.0,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "slow committee",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 625_000.0,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "curvy twig",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 625_000.0,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "gamy scissors",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 250_000.0,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "mushy key",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 1_250_000.0,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "marked silver",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 250_000.0,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "free sock",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 625_000.0,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "tremendous meeting",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 1_250_000.0,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "panoramic cloth",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 625_000.0,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "normal kick",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 2_500_000.0,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "unbecoming observation",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 250_000.0,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "cut beginner",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 250_000.0,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "alcoholic button",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 625_000.0,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "old-fashioned clover",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 750_000.0,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "expensive underwear",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 2_500_000.0,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "like dust",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 5_000_000.0,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "rapid straw",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 5_850_000.0,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "windy trousers",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 2_579_350.0,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "dramatic veil",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 3_611_110.50,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "incandescent skin",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 3_000_000.0,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "spiky love",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 3_250_000.0,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
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
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 4_000_000.0,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "alcoholic letter",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 4_000_000.0,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "heady trucks",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 4_000_000.0,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "ten support",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 1_000_000.0,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "foregoing middle",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 800_000.0,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "ludicrous destruction",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 4_000_000.0,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "numberless wheel",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 4_000_000.0,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "short powder",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 4_000_000.0,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "cut name",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 4_000_000.0,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "six fly",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 4_000_000.0,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "mindless pickle",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 100_000.0,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "marked rabbit",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 38_741.36,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "jagged doctor",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 711_258.64,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "truthful pollution",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 1_587_300.0,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "unkempt activity",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 2_222_220.0,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "ritzy view",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 40_000.0,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "remarkable plant",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 300_000.0,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "busy value",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 100_000.0,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "imperfect slave",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 222_065.84,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "uneven drawer",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 400_000.0,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "far behavior",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 4_000_000.0,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "abaft memory",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 400_000.0,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "poor glove",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 2_000_000.0,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "strange iron",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 2_000_000.0,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "nonstop rail",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 1_000_000.0,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "milky bait",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 400_000.0,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "wandering start",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 1_200_000.0,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
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
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 1_212_121.21,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "godly bed",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 151_515.15,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "innocent property",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 227_272.73,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "responsible bikes",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 3_030_303.03,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "learned market",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 3_030_303.03,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "jumpy school",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 303_030.30,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "sticky houses",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 1_515_151.52,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "bustling basketball",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 1_515_152.52,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "ordinary dad",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 606_060.61,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "absurd bat",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 90_909.09,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "cloudy ocean",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 67_945.45,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "black-and-white fold",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 757_575.76,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "stale part",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 45_454.55,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "available health",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 2_797_575.76,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "afraid visitor",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 481_818.18,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "arrogant front",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 151_515.15,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "juvenile zinc",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 151_515.15,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "disturbed box",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 303_030.30,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "disagreeable skate",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 454_545.45,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "miscreant sidewalk",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 75_757.58,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
    },
    StakerInfo {
        name: "shy play",
        staker: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        withdrawer: "cafebabedeadbeef000000000000000000000000000000000000000000000000",
        sol: 303_030.30,
        custodian: "0000000000000000000000000000000000000000000000000000000000000000",
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

pub(crate) fn add_genesis_accounts(genesis_config: &mut GenesisConfig) -> u64 {
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
    )
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

        genesis_config
            .accounts
            .sort_by(|(ka, _), (kb, _)| ka.cmp(kb));

        let len = genesis_config.accounts.len();
        genesis_config
            .accounts
            .dedup_by(|(ka, _), (kb, _)| ka == kb);
        assert_eq!(genesis_config.accounts.len(), len);
    }
}
