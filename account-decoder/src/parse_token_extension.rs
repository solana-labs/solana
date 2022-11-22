use {
    crate::parse_token::UiAccountState,
    solana_sdk::clock::UnixTimestamp,
    spl_token_2022::{
        extension::{self, BaseState, BaseStateWithExtensions, ExtensionType, StateWithExtensions},
        solana_program::pubkey::Pubkey,
    },
};

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
    UnparseableExtension,
}

pub fn parse_extension<S: BaseState>(
    extension_type: &ExtensionType,
    account: &StateWithExtensions<S>,
) -> UiExtension {
    match &extension_type {
        ExtensionType::Uninitialized => UiExtension::Uninitialized,
        ExtensionType::TransferFeeConfig => account
            .get_extension::<extension::transfer_fee::TransferFeeConfig>()
            .map(|&extension| UiExtension::TransferFeeConfig(extension.into()))
            .unwrap_or(UiExtension::UnparseableExtension),
        ExtensionType::TransferFeeAmount => account
            .get_extension::<extension::transfer_fee::TransferFeeAmount>()
            .map(|&extension| UiExtension::TransferFeeAmount(extension.into()))
            .unwrap_or(UiExtension::UnparseableExtension),
        ExtensionType::MintCloseAuthority => account
            .get_extension::<extension::mint_close_authority::MintCloseAuthority>()
            .map(|&extension| UiExtension::MintCloseAuthority(extension.into()))
            .unwrap_or(UiExtension::UnparseableExtension),
        ExtensionType::ConfidentialTransferMint => account
            .get_extension::<extension::confidential_transfer::ConfidentialTransferMint>()
            .map(|&extension| UiExtension::ConfidentialTransferMint(extension.into()))
            .unwrap_or(UiExtension::UnparseableExtension),
        ExtensionType::ConfidentialTransferAccount => account
            .get_extension::<extension::confidential_transfer::ConfidentialTransferAccount>()
            .map(|&extension| UiExtension::ConfidentialTransferAccount(extension.into()))
            .unwrap_or(UiExtension::UnparseableExtension),
        ExtensionType::DefaultAccountState => account
            .get_extension::<extension::default_account_state::DefaultAccountState>()
            .map(|&extension| UiExtension::DefaultAccountState(extension.into()))
            .unwrap_or(UiExtension::UnparseableExtension),
        ExtensionType::ImmutableOwner => UiExtension::ImmutableOwner,
        ExtensionType::MemoTransfer => account
            .get_extension::<extension::memo_transfer::MemoTransfer>()
            .map(|&extension| UiExtension::MemoTransfer(extension.into()))
            .unwrap_or(UiExtension::UnparseableExtension),
        ExtensionType::NonTransferable => UiExtension::NonTransferable,
        ExtensionType::InterestBearingConfig => account
            .get_extension::<extension::interest_bearing_mint::InterestBearingConfig>()
            .map(|&extension| UiExtension::InterestBearingConfig(extension.into()))
            .unwrap_or(UiExtension::UnparseableExtension),
        ExtensionType::CpiGuard => account
            .get_extension::<extension::cpi_guard::CpiGuard>()
            .map(|&extension| UiExtension::CpiGuard(extension.into()))
            .unwrap_or(UiExtension::UnparseableExtension),
        ExtensionType::PermanentDelegate => account
            .get_extension::<extension::permanent_delegate::PermanentDelegate>()
            .map(|&extension| UiExtension::PermanentDelegate(extension.into()))
            .unwrap_or(UiExtension::UnparseableExtension),
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiTransferFee {
    pub epoch: u64,
    pub maximum_fee: u64,
    pub transfer_fee_basis_points: u16,
}

impl From<extension::transfer_fee::TransferFee> for UiTransferFee {
    fn from(transfer_fee: extension::transfer_fee::TransferFee) -> Self {
        Self {
            epoch: u64::from(transfer_fee.epoch),
            maximum_fee: u64::from(transfer_fee.maximum_fee),
            transfer_fee_basis_points: u16::from(transfer_fee.transfer_fee_basis_points),
        }
    }
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

impl From<extension::transfer_fee::TransferFeeConfig> for UiTransferFeeConfig {
    fn from(transfer_fee_config: extension::transfer_fee::TransferFeeConfig) -> Self {
        let transfer_fee_config_authority: Option<Pubkey> =
            transfer_fee_config.transfer_fee_config_authority.into();
        let withdraw_withheld_authority: Option<Pubkey> =
            transfer_fee_config.withdraw_withheld_authority.into();

        Self {
            transfer_fee_config_authority: transfer_fee_config_authority
                .map(|pubkey| pubkey.to_string()),
            withdraw_withheld_authority: withdraw_withheld_authority
                .map(|pubkey| pubkey.to_string()),
            withheld_amount: u64::from(transfer_fee_config.withheld_amount),
            older_transfer_fee: transfer_fee_config.older_transfer_fee.into(),
            newer_transfer_fee: transfer_fee_config.newer_transfer_fee.into(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiTransferFeeAmount {
    pub withheld_amount: u64,
}

impl From<extension::transfer_fee::TransferFeeAmount> for UiTransferFeeAmount {
    fn from(transfer_fee_amount: extension::transfer_fee::TransferFeeAmount) -> Self {
        Self {
            withheld_amount: u64::from(transfer_fee_amount.withheld_amount),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiMintCloseAuthority {
    pub close_authority: Option<String>,
}

impl From<extension::mint_close_authority::MintCloseAuthority> for UiMintCloseAuthority {
    fn from(mint_close_authority: extension::mint_close_authority::MintCloseAuthority) -> Self {
        let authority: Option<Pubkey> = mint_close_authority.close_authority.into();
        Self {
            close_authority: authority.map(|pubkey| pubkey.to_string()),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiDefaultAccountState {
    pub account_state: UiAccountState,
}

impl From<extension::default_account_state::DefaultAccountState> for UiDefaultAccountState {
    fn from(default_account_state: extension::default_account_state::DefaultAccountState) -> Self {
        let account_state =
            spl_token_2022::state::AccountState::try_from(default_account_state.state)
                .unwrap_or_default();
        Self {
            account_state: account_state.into(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiMemoTransfer {
    pub require_incoming_transfer_memos: bool,
}

impl From<extension::memo_transfer::MemoTransfer> for UiMemoTransfer {
    fn from(memo_transfer: extension::memo_transfer::MemoTransfer) -> Self {
        Self {
            require_incoming_transfer_memos: memo_transfer.require_incoming_transfer_memos.into(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiInterestBearingConfig {
    pub rate_authority: Option<String>,
    pub initialization_timestamp: UnixTimestamp,
    pub pre_update_average_rate: i16,
    pub last_update_timestamp: UnixTimestamp,
    pub current_rate: i16,
}

impl From<extension::interest_bearing_mint::InterestBearingConfig> for UiInterestBearingConfig {
    fn from(
        interest_bearing_config: extension::interest_bearing_mint::InterestBearingConfig,
    ) -> Self {
        let rate_authority: Option<Pubkey> = interest_bearing_config.rate_authority.into();

        Self {
            rate_authority: rate_authority.map(|pubkey| pubkey.to_string()),
            initialization_timestamp: UnixTimestamp::from(
                interest_bearing_config.initialization_timestamp,
            ),
            pre_update_average_rate: i16::from(interest_bearing_config.pre_update_average_rate),
            last_update_timestamp: UnixTimestamp::from(
                interest_bearing_config.last_update_timestamp,
            ),
            current_rate: i16::from(interest_bearing_config.current_rate),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiCpiGuard {
    pub lock_cpi: bool,
}

impl From<extension::cpi_guard::CpiGuard> for UiCpiGuard {
    fn from(cpi_guard: extension::cpi_guard::CpiGuard) -> Self {
        Self {
            lock_cpi: cpi_guard.lock_cpi.into(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiPermanentDelegate {
    pub delegate: Option<String>,
}

impl From<extension::permanent_delegate::PermanentDelegate> for UiPermanentDelegate {
    fn from(permanent_delegate: extension::permanent_delegate::PermanentDelegate) -> Self {
        let delegate: Option<Pubkey> = permanent_delegate.delegate.into();
        Self {
            delegate: delegate.map(|pubkey| pubkey.to_string()),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiConfidentialTransferMint {
    pub authority: String,
    pub auto_approve_new_accounts: bool,
    pub auditor_encryption_pubkey: String,
    pub withdraw_withheld_authority_encryption_pubkey: String,
    pub withheld_amount: String,
}

impl From<extension::confidential_transfer::ConfidentialTransferMint>
    for UiConfidentialTransferMint
{
    fn from(
        confidential_transfer_mint: extension::confidential_transfer::ConfidentialTransferMint,
    ) -> Self {
        Self {
            authority: confidential_transfer_mint.authority.to_string(),
            auto_approve_new_accounts: confidential_transfer_mint.auto_approve_new_accounts.into(),
            auditor_encryption_pubkey: format!(
                "{}",
                confidential_transfer_mint.auditor_encryption_pubkey
            ),
            withdraw_withheld_authority_encryption_pubkey: format!(
                "{}",
                confidential_transfer_mint.withdraw_withheld_authority_encryption_pubkey
            ),
            withheld_amount: format!("{}", confidential_transfer_mint.withheld_amount),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiConfidentialTransferAccount {
    pub approved: bool,
    pub encryption_pubkey: String,
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
    pub withheld_amount: String,
}

impl From<extension::confidential_transfer::ConfidentialTransferAccount>
    for UiConfidentialTransferAccount
{
    fn from(
        confidential_transfer_account: extension::confidential_transfer::ConfidentialTransferAccount,
    ) -> Self {
        Self {
            approved: confidential_transfer_account.approved.into(),
            encryption_pubkey: format!("{}", confidential_transfer_account.encryption_pubkey),
            pending_balance_lo: format!("{}", confidential_transfer_account.pending_balance_lo),
            pending_balance_hi: format!("{}", confidential_transfer_account.pending_balance_hi),
            available_balance: format!("{}", confidential_transfer_account.available_balance),
            decryptable_available_balance: format!(
                "{}",
                confidential_transfer_account.decryptable_available_balance
            ),
            allow_confidential_credits: confidential_transfer_account
                .allow_confidential_credits
                .into(),
            allow_non_confidential_credits: confidential_transfer_account
                .allow_non_confidential_credits
                .into(),
            pending_balance_credit_counter: confidential_transfer_account
                .pending_balance_credit_counter
                .into(),
            maximum_pending_balance_credit_counter: confidential_transfer_account
                .maximum_pending_balance_credit_counter
                .into(),
            expected_pending_balance_credit_counter: confidential_transfer_account
                .expected_pending_balance_credit_counter
                .into(),
            actual_pending_balance_credit_counter: confidential_transfer_account
                .actual_pending_balance_credit_counter
                .into(),
            withheld_amount: format!("{}", confidential_transfer_account.withheld_amount),
        }
    }
}
