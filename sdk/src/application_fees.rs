#![cfg(feature = "full")]
crate::declare_id!("App1icationFees1111111111111111111111111111");

#[derive(Serialize, Deserialize, Clone, Copy)]
pub struct ApplicationFeeStructure {
    pub fee_lamports: u64,
    pub version: u32,
}
