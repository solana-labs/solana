pub mod nft_instruction;
pub mod nft_processor;

const NFT_PROGRAM_ID: [u8; 32] = [
    5, 113, 136, 161, 25, 86, 254, 18, 254, 185, 151, 222, 110, 23, 247, 97, 48, 219, 90, 202, 33,
    26, 213, 111, 208, 248, 122, 0, 0, 0, 0, 0,
];

solana_sdk::solana_name_id!(
    NFT_PROGRAM_ID,
    "NFT1111111111111111111111111111111111111111"
);
