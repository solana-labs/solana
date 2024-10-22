pub use solana_account_decoder_client_types::token::{
    UiConfidentialTransferAccount, UiConfidentialTransferFeeAmount,
    UiConfidentialTransferFeeConfig, UiConfidentialTransferMint, UiCpiGuard, UiDefaultAccountState,
    UiExtension, UiGroupMemberPointer, UiGroupPointer, UiInterestBearingConfig, UiMemoTransfer,
    UiMetadataPointer, UiMintCloseAuthority, UiPermanentDelegate, UiTokenGroup, UiTokenGroupMember,
    UiTokenMetadata, UiTransferFee, UiTransferFeeAmount, UiTransferFeeConfig, UiTransferHook,
    UiTransferHookAccount,
};
use {
    crate::parse_token::convert_account_state,
    solana_sdk::{clock::UnixTimestamp, program_pack::Pack},
    spl_token_2022::{
        extension::{self, BaseState, BaseStateWithExtensions, ExtensionType, StateWithExtensions},
        solana_program::pubkey::Pubkey,
        solana_zk_token_sdk::zk_token_elgamal::pod::ElGamalPubkey,
    },
    spl_token_group_interface::state::{TokenGroup, TokenGroupMember},
    spl_token_metadata_interface::state::TokenMetadata,
};

pub fn parse_extension<S: BaseState + Pack>(
    extension_type: &ExtensionType,
    account: &StateWithExtensions<S>,
) -> UiExtension {
    match extension_type {
        ExtensionType::Uninitialized => UiExtension::Uninitialized,
        ExtensionType::TransferFeeConfig => account
            .get_extension::<extension::transfer_fee::TransferFeeConfig>()
            .map(|&extension| {
                UiExtension::TransferFeeConfig(convert_transfer_fee_config(extension))
            })
            .unwrap_or(UiExtension::UnparseableExtension),
        ExtensionType::TransferFeeAmount => account
            .get_extension::<extension::transfer_fee::TransferFeeAmount>()
            .map(|&extension| {
                UiExtension::TransferFeeAmount(convert_transfer_fee_amount(extension))
            })
            .unwrap_or(UiExtension::UnparseableExtension),
        ExtensionType::MintCloseAuthority => account
            .get_extension::<extension::mint_close_authority::MintCloseAuthority>()
            .map(|&extension| {
                UiExtension::MintCloseAuthority(convert_mint_close_authority(extension))
            })
            .unwrap_or(UiExtension::UnparseableExtension),
        ExtensionType::ConfidentialTransferMint => account
            .get_extension::<extension::confidential_transfer::ConfidentialTransferMint>()
            .map(|&extension| {
                UiExtension::ConfidentialTransferMint(convert_confidential_transfer_mint(extension))
            })
            .unwrap_or(UiExtension::UnparseableExtension),
        ExtensionType::ConfidentialTransferFeeConfig => account
            .get_extension::<extension::confidential_transfer_fee::ConfidentialTransferFeeConfig>()
            .map(|&extension| {
                UiExtension::ConfidentialTransferFeeConfig(
                    convert_confidential_transfer_fee_config(extension),
                )
            })
            .unwrap_or(UiExtension::UnparseableExtension),
        ExtensionType::ConfidentialTransferAccount => account
            .get_extension::<extension::confidential_transfer::ConfidentialTransferAccount>()
            .map(|&extension| {
                UiExtension::ConfidentialTransferAccount(convert_confidential_transfer_account(
                    extension,
                ))
            })
            .unwrap_or(UiExtension::UnparseableExtension),
        ExtensionType::ConfidentialTransferFeeAmount => account
            .get_extension::<extension::confidential_transfer_fee::ConfidentialTransferFeeAmount>()
            .map(|&extension| {
                UiExtension::ConfidentialTransferFeeAmount(
                    convert_confidential_transfer_fee_amount(extension),
                )
            })
            .unwrap_or(UiExtension::UnparseableExtension),
        ExtensionType::DefaultAccountState => account
            .get_extension::<extension::default_account_state::DefaultAccountState>()
            .map(|&extension| {
                UiExtension::DefaultAccountState(convert_default_account_state(extension))
            })
            .unwrap_or(UiExtension::UnparseableExtension),
        ExtensionType::ImmutableOwner => UiExtension::ImmutableOwner,
        ExtensionType::MemoTransfer => account
            .get_extension::<extension::memo_transfer::MemoTransfer>()
            .map(|&extension| UiExtension::MemoTransfer(convert_memo_transfer(extension)))
            .unwrap_or(UiExtension::UnparseableExtension),
        ExtensionType::NonTransferable => UiExtension::NonTransferable,
        ExtensionType::InterestBearingConfig => account
            .get_extension::<extension::interest_bearing_mint::InterestBearingConfig>()
            .map(|&extension| {
                UiExtension::InterestBearingConfig(convert_interest_bearing_config(extension))
            })
            .unwrap_or(UiExtension::UnparseableExtension),
        ExtensionType::CpiGuard => account
            .get_extension::<extension::cpi_guard::CpiGuard>()
            .map(|&extension| UiExtension::CpiGuard(convert_cpi_guard(extension)))
            .unwrap_or(UiExtension::UnparseableExtension),
        ExtensionType::PermanentDelegate => account
            .get_extension::<extension::permanent_delegate::PermanentDelegate>()
            .map(|&extension| UiExtension::PermanentDelegate(convert_permanent_delegate(extension)))
            .unwrap_or(UiExtension::UnparseableExtension),
        ExtensionType::NonTransferableAccount => UiExtension::NonTransferableAccount,
        ExtensionType::MetadataPointer => account
            .get_extension::<extension::metadata_pointer::MetadataPointer>()
            .map(|&extension| UiExtension::MetadataPointer(convert_metadata_pointer(extension)))
            .unwrap_or(UiExtension::UnparseableExtension),
        ExtensionType::TokenMetadata => account
            .get_variable_len_extension::<TokenMetadata>()
            .map(|extension| UiExtension::TokenMetadata(convert_token_metadata(extension)))
            .unwrap_or(UiExtension::UnparseableExtension),
        ExtensionType::TransferHook => account
            .get_extension::<extension::transfer_hook::TransferHook>()
            .map(|&extension| UiExtension::TransferHook(convert_transfer_hook(extension)))
            .unwrap_or(UiExtension::UnparseableExtension),
        ExtensionType::TransferHookAccount => account
            .get_extension::<extension::transfer_hook::TransferHookAccount>()
            .map(|&extension| {
                UiExtension::TransferHookAccount(convert_transfer_hook_account(extension))
            })
            .unwrap_or(UiExtension::UnparseableExtension),
        ExtensionType::GroupPointer => account
            .get_extension::<extension::group_pointer::GroupPointer>()
            .map(|&extension| UiExtension::GroupPointer(convert_group_pointer(extension)))
            .unwrap_or(UiExtension::UnparseableExtension),
        ExtensionType::GroupMemberPointer => account
            .get_extension::<extension::group_member_pointer::GroupMemberPointer>()
            .map(|&extension| {
                UiExtension::GroupMemberPointer(convert_group_member_pointer(extension))
            })
            .unwrap_or(UiExtension::UnparseableExtension),
        ExtensionType::TokenGroup => account
            .get_extension::<TokenGroup>()
            .map(|&extension| UiExtension::TokenGroup(convert_token_group(extension)))
            .unwrap_or(UiExtension::UnparseableExtension),
        ExtensionType::TokenGroupMember => account
            .get_extension::<TokenGroupMember>()
            .map(|&extension| UiExtension::TokenGroupMember(convert_token_group_member(extension)))
            .unwrap_or(UiExtension::UnparseableExtension),
    }
}

fn convert_transfer_fee(transfer_fee: extension::transfer_fee::TransferFee) -> UiTransferFee {
    UiTransferFee {
        epoch: u64::from(transfer_fee.epoch),
        maximum_fee: u64::from(transfer_fee.maximum_fee),
        transfer_fee_basis_points: u16::from(transfer_fee.transfer_fee_basis_points),
    }
}

fn convert_transfer_fee_config(
    transfer_fee_config: extension::transfer_fee::TransferFeeConfig,
) -> UiTransferFeeConfig {
    let transfer_fee_config_authority: Option<Pubkey> =
        transfer_fee_config.transfer_fee_config_authority.into();
    let withdraw_withheld_authority: Option<Pubkey> =
        transfer_fee_config.withdraw_withheld_authority.into();

    UiTransferFeeConfig {
        transfer_fee_config_authority: transfer_fee_config_authority
            .map(|pubkey| pubkey.to_string()),
        withdraw_withheld_authority: withdraw_withheld_authority.map(|pubkey| pubkey.to_string()),
        withheld_amount: u64::from(transfer_fee_config.withheld_amount),
        older_transfer_fee: convert_transfer_fee(transfer_fee_config.older_transfer_fee),
        newer_transfer_fee: convert_transfer_fee(transfer_fee_config.newer_transfer_fee),
    }
}

fn convert_transfer_fee_amount(
    transfer_fee_amount: extension::transfer_fee::TransferFeeAmount,
) -> UiTransferFeeAmount {
    UiTransferFeeAmount {
        withheld_amount: u64::from(transfer_fee_amount.withheld_amount),
    }
}

fn convert_mint_close_authority(
    mint_close_authority: extension::mint_close_authority::MintCloseAuthority,
) -> UiMintCloseAuthority {
    let authority: Option<Pubkey> = mint_close_authority.close_authority.into();
    UiMintCloseAuthority {
        close_authority: authority.map(|pubkey| pubkey.to_string()),
    }
}

fn convert_default_account_state(
    default_account_state: extension::default_account_state::DefaultAccountState,
) -> UiDefaultAccountState {
    let account_state = spl_token_2022::state::AccountState::try_from(default_account_state.state)
        .unwrap_or_default();
    UiDefaultAccountState {
        account_state: convert_account_state(account_state),
    }
}

fn convert_memo_transfer(memo_transfer: extension::memo_transfer::MemoTransfer) -> UiMemoTransfer {
    UiMemoTransfer {
        require_incoming_transfer_memos: memo_transfer.require_incoming_transfer_memos.into(),
    }
}

fn convert_interest_bearing_config(
    interest_bearing_config: extension::interest_bearing_mint::InterestBearingConfig,
) -> UiInterestBearingConfig {
    let rate_authority: Option<Pubkey> = interest_bearing_config.rate_authority.into();

    UiInterestBearingConfig {
        rate_authority: rate_authority.map(|pubkey| pubkey.to_string()),
        initialization_timestamp: UnixTimestamp::from(
            interest_bearing_config.initialization_timestamp,
        ),
        pre_update_average_rate: i16::from(interest_bearing_config.pre_update_average_rate),
        last_update_timestamp: UnixTimestamp::from(interest_bearing_config.last_update_timestamp),
        current_rate: i16::from(interest_bearing_config.current_rate),
    }
}

fn convert_cpi_guard(cpi_guard: extension::cpi_guard::CpiGuard) -> UiCpiGuard {
    UiCpiGuard {
        lock_cpi: cpi_guard.lock_cpi.into(),
    }
}

fn convert_permanent_delegate(
    permanent_delegate: extension::permanent_delegate::PermanentDelegate,
) -> UiPermanentDelegate {
    let delegate: Option<Pubkey> = permanent_delegate.delegate.into();
    UiPermanentDelegate {
        delegate: delegate.map(|pubkey| pubkey.to_string()),
    }
}

pub fn convert_confidential_transfer_mint(
    confidential_transfer_mint: extension::confidential_transfer::ConfidentialTransferMint,
) -> UiConfidentialTransferMint {
    let authority: Option<Pubkey> = confidential_transfer_mint.authority.into();
    let auditor_elgamal_pubkey: Option<ElGamalPubkey> =
        confidential_transfer_mint.auditor_elgamal_pubkey.into();
    UiConfidentialTransferMint {
        authority: authority.map(|pubkey| pubkey.to_string()),
        auto_approve_new_accounts: confidential_transfer_mint.auto_approve_new_accounts.into(),
        auditor_elgamal_pubkey: auditor_elgamal_pubkey.map(|pubkey| pubkey.to_string()),
    }
}

pub fn convert_confidential_transfer_fee_config(
    confidential_transfer_fee_config: extension::confidential_transfer_fee::ConfidentialTransferFeeConfig,
) -> UiConfidentialTransferFeeConfig {
    let authority: Option<Pubkey> = confidential_transfer_fee_config.authority.into();
    let withdraw_withheld_authority_elgamal_pubkey: Option<ElGamalPubkey> =
        confidential_transfer_fee_config
            .withdraw_withheld_authority_elgamal_pubkey
            .into();
    UiConfidentialTransferFeeConfig {
        authority: authority.map(|pubkey| pubkey.to_string()),
        withdraw_withheld_authority_elgamal_pubkey: withdraw_withheld_authority_elgamal_pubkey
            .map(|pubkey| pubkey.to_string()),
        harvest_to_mint_enabled: confidential_transfer_fee_config
            .harvest_to_mint_enabled
            .into(),
        withheld_amount: format!("{}", confidential_transfer_fee_config.withheld_amount),
    }
}

fn convert_confidential_transfer_account(
    confidential_transfer_account: extension::confidential_transfer::ConfidentialTransferAccount,
) -> UiConfidentialTransferAccount {
    UiConfidentialTransferAccount {
        approved: confidential_transfer_account.approved.into(),
        elgamal_pubkey: format!("{}", confidential_transfer_account.elgamal_pubkey),
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
    }
}

fn convert_confidential_transfer_fee_amount(
    confidential_transfer_fee_amount: extension::confidential_transfer_fee::ConfidentialTransferFeeAmount,
) -> UiConfidentialTransferFeeAmount {
    UiConfidentialTransferFeeAmount {
        withheld_amount: format!("{}", confidential_transfer_fee_amount.withheld_amount),
    }
}

fn convert_metadata_pointer(
    metadata_pointer: extension::metadata_pointer::MetadataPointer,
) -> UiMetadataPointer {
    let authority: Option<Pubkey> = metadata_pointer.authority.into();
    let metadata_address: Option<Pubkey> = metadata_pointer.metadata_address.into();
    UiMetadataPointer {
        authority: authority.map(|pubkey| pubkey.to_string()),
        metadata_address: metadata_address.map(|pubkey| pubkey.to_string()),
    }
}

fn convert_token_metadata(token_metadata: TokenMetadata) -> UiTokenMetadata {
    let update_authority: Option<Pubkey> = token_metadata.update_authority.into();
    UiTokenMetadata {
        update_authority: update_authority.map(|pubkey| pubkey.to_string()),
        mint: token_metadata.mint.to_string(),
        name: token_metadata.name,
        symbol: token_metadata.symbol,
        uri: token_metadata.uri,
        additional_metadata: token_metadata.additional_metadata,
    }
}

fn convert_transfer_hook(transfer_hook: extension::transfer_hook::TransferHook) -> UiTransferHook {
    let authority: Option<Pubkey> = transfer_hook.authority.into();
    let program_id: Option<Pubkey> = transfer_hook.program_id.into();
    UiTransferHook {
        authority: authority.map(|pubkey| pubkey.to_string()),
        program_id: program_id.map(|pubkey| pubkey.to_string()),
    }
}

fn convert_transfer_hook_account(
    transfer_hook: extension::transfer_hook::TransferHookAccount,
) -> UiTransferHookAccount {
    UiTransferHookAccount {
        transferring: transfer_hook.transferring.into(),
    }
}

fn convert_group_pointer(group_pointer: extension::group_pointer::GroupPointer) -> UiGroupPointer {
    let authority: Option<Pubkey> = group_pointer.authority.into();
    let group_address: Option<Pubkey> = group_pointer.group_address.into();
    UiGroupPointer {
        authority: authority.map(|pubkey| pubkey.to_string()),
        group_address: group_address.map(|pubkey| pubkey.to_string()),
    }
}

fn convert_group_member_pointer(
    member_pointer: extension::group_member_pointer::GroupMemberPointer,
) -> UiGroupMemberPointer {
    let authority: Option<Pubkey> = member_pointer.authority.into();
    let member_address: Option<Pubkey> = member_pointer.member_address.into();
    UiGroupMemberPointer {
        authority: authority.map(|pubkey| pubkey.to_string()),
        member_address: member_address.map(|pubkey| pubkey.to_string()),
    }
}

fn convert_token_group(token_group: TokenGroup) -> UiTokenGroup {
    let update_authority: Option<Pubkey> = token_group.update_authority.into();
    UiTokenGroup {
        update_authority: update_authority.map(|pubkey| pubkey.to_string()),
        mint: token_group.mint.to_string(),
        size: token_group.size.into(),
        max_size: token_group.max_size.into(),
    }
}

fn convert_token_group_member(member: TokenGroupMember) -> UiTokenGroupMember {
    UiTokenGroupMember {
        mint: member.mint.to_string(),
        group: member.group.to_string(),
        member_number: member.member_number.into(),
    }
}
