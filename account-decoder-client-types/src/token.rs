use {
    core::str::FromStr,
    serde_derive::{Deserialize, Serialize},
};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UiTokenAmount {
    pub ui_amount: Option<f64>,
    pub decimals: u8,
    pub amount: String,
    pub ui_amount_string: String,
}

impl UiTokenAmount {
    pub fn real_number_string(&self) -> String {
        real_number_string(
            u64::from_str(&self.amount).unwrap_or_default(),
            self.decimals,
        )
    }

    pub fn real_number_string_trimmed(&self) -> String {
        if !self.ui_amount_string.is_empty() {
            self.ui_amount_string.clone()
        } else {
            real_number_string_trimmed(
                u64::from_str(&self.amount).unwrap_or_default(),
                self.decimals,
            )
        }
    }
}

#[allow(clippy::arithmetic_side_effects)]
pub fn real_number_string(amount: u64, decimals: u8) -> String {
    let decimals = decimals as usize;
    if decimals > 0 {
        // Left-pad zeros to decimals + 1, so we at least have an integer zero
        let mut s = format!("{:01$}", amount, decimals + 1);
        // Add the decimal point (Sorry, "," locales!)
        s.insert(s.len() - decimals, '.');
        s
    } else {
        amount.to_string()
    }
}

pub fn real_number_string_trimmed(amount: u64, decimals: u8) -> String {
    let mut s = real_number_string(amount, decimals);
    if decimals > 0 {
        let zeros_trimmed = s.trim_end_matches('0');
        s = zeros_trimmed.trim_end_matches('.').to_string();
    }
    s
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", tag = "type", content = "info")]
#[allow(clippy::large_enum_variant)]
pub enum TokenAccountType {
    Account(UiTokenAccount),
    Mint(UiMint),
    Multisig(UiMultisig),
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UiTokenAccount {
    pub mint: String,
    pub owner: String,
    pub token_amount: UiTokenAmount,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delegate: Option<String>,
    pub state: UiAccountState,
    pub is_native: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rent_exempt_reserve: Option<UiTokenAmount>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delegated_amount: Option<UiTokenAmount>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub close_authority: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub extensions: Vec<UiExtension>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum UiAccountState {
    Uninitialized,
    Initialized,
    Frozen,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", tag = "extension", content = "state")]
pub enum UiExtension {
    Uninitialized,
    TransferFeeConfig(UiTransferFeeConfig),
    TransferFeeAmount(UiTransferFeeAmount),
    MintCloseAuthority(UiMintCloseAuthority),
    ConfidentialTransferMint(UiConfidentialTransferMint),
    ConfidentialTransferAccount(UiConfidentialTransferAccount),
    DefaultAccountState(UiDefaultAccountState),
    ImmutableOwner,
    MemoTransfer(UiMemoTransfer),
    NonTransferable,
    InterestBearingConfig(UiInterestBearingConfig),
    CpiGuard(UiCpiGuard),
    PermanentDelegate(UiPermanentDelegate),
    NonTransferableAccount,
    ConfidentialTransferFeeConfig(UiConfidentialTransferFeeConfig),
    ConfidentialTransferFeeAmount(UiConfidentialTransferFeeAmount),
    TransferHook(UiTransferHook),
    TransferHookAccount(UiTransferHookAccount),
    MetadataPointer(UiMetadataPointer),
    TokenMetadata(UiTokenMetadata),
    GroupPointer(UiGroupPointer),
    GroupMemberPointer(UiGroupMemberPointer),
    TokenGroup(UiTokenGroup),
    TokenGroupMember(UiTokenGroupMember),
    UnparseableExtension,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiTokenGroupMember {
    pub mint: String,
    pub group: String,
    pub member_number: u32,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiTokenGroup {
    pub update_authority: Option<String>,
    pub mint: String,
    pub size: u32,
    pub max_size: u32,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiMetadataPointer {
    pub authority: Option<String>,
    pub metadata_address: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiTransferHook {
    pub authority: Option<String>,
    pub program_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiTransferHookAccount {
    pub transferring: bool,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiConfidentialTransferMint {
    pub authority: Option<String>,
    pub auto_approve_new_accounts: bool,
    pub auditor_elgamal_pubkey: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiConfidentialTransferFeeAmount {
    pub withheld_amount: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiConfidentialTransferFeeConfig {
    pub authority: Option<String>,
    pub withdraw_withheld_authority_elgamal_pubkey: Option<String>,
    pub harvest_to_mint_enabled: bool,
    pub withheld_amount: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiPermanentDelegate {
    pub delegate: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiCpiGuard {
    pub lock_cpi: bool,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiInterestBearingConfig {
    pub rate_authority: Option<String>,
    pub initialization_timestamp: i64,
    pub pre_update_average_rate: i16,
    pub last_update_timestamp: i64,
    pub current_rate: i16,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiMemoTransfer {
    pub require_incoming_transfer_memos: bool,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiDefaultAccountState {
    pub account_state: UiAccountState,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiConfidentialTransferAccount {
    pub approved: bool,
    pub elgamal_pubkey: String,
    pub pending_balance_lo: String,
    pub pending_balance_hi: String,
    pub available_balance: String,
    pub decryptable_available_balance: String,
    pub allow_confidential_credits: bool,
    pub allow_non_confidential_credits: bool,
    pub pending_balance_credit_counter: u64,
    pub maximum_pending_balance_credit_counter: u64,
    pub expected_pending_balance_credit_counter: u64,
    pub actual_pending_balance_credit_counter: u64,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiMintCloseAuthority {
    pub close_authority: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiTransferFeeAmount {
    pub withheld_amount: u64,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiTransferFeeConfig {
    pub transfer_fee_config_authority: Option<String>,
    pub withdraw_withheld_authority: Option<String>,
    pub withheld_amount: u64,
    pub older_transfer_fee: UiTransferFee,
    pub newer_transfer_fee: UiTransferFee,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiTransferFee {
    pub epoch: u64,
    pub maximum_fee: u64,
    pub transfer_fee_basis_points: u16,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiGroupMemberPointer {
    pub authority: Option<String>,
    pub member_address: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiGroupPointer {
    pub authority: Option<String>,
    pub group_address: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiTokenMetadata {
    pub update_authority: Option<String>,
    pub mint: String,
    pub name: String,
    pub symbol: String,
    pub uri: String,
    pub additional_metadata: Vec<(String, String)>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiMint {
    pub mint_authority: Option<String>,
    pub supply: String,
    pub decimals: u8,
    pub is_initialized: bool,
    pub freeze_authority: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub extensions: Vec<UiExtension>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiMultisig {
    pub num_required_signers: u8,
    pub num_valid_signers: u8,
    pub is_initialized: bool,
    pub signers: Vec<String>,
}
