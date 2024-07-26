use {solana_sdk::fee::FeeDetails, solana_svm_transaction::svm_message::SVMMessage};

/// Calculate fee for `SanitizedMessage`
pub fn calculate_fee(
    message: &impl SVMMessage,
    zero_fees_for_test: bool,
    lamports_per_signature: u64,
    prioritization_fee: u64,
    remove_rounding_in_fee_calculation: bool,
) -> u64 {
    calculate_fee_details(
        message,
        zero_fees_for_test,
        lamports_per_signature,
        prioritization_fee,
        remove_rounding_in_fee_calculation,
    )
    .total_fee()
}

pub fn calculate_fee_details(
    message: &impl SVMMessage,
    zero_fees_for_test: bool,
    lamports_per_signature: u64,
    prioritization_fee: u64,
    remove_rounding_in_fee_calculation: bool,
) -> FeeDetails {
    if zero_fees_for_test {
        return FeeDetails::default();
    }
    let signature_fee = message
        .num_total_signatures()
        .saturating_mul(lamports_per_signature);

    FeeDetails::new(
        signature_fee,
        prioritization_fee,
        remove_rounding_in_fee_calculation,
    )
}
