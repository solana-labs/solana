//! Utility functions

/// Helper function for programs to call [`GetMinimumDelegation`] and then fetch the return data
///
/// This fn handles performing the CPI to call the [`GetMinimumDelegation`] function, and then
/// calls [`get_return_data()`] to fetch the return data.
///
/// [`GetMinimumDelegation`]: super::instruction::StakeInstruction::GetMinimumDelegation
/// [`get_return_data()`]: crate::program::get_return_data
pub fn get_minimum_delegation() -> u64 {
    let instruction = super::instruction::get_minimum_delegation();
    // SAFETY: The `.unwrap()` is safe because `invoke_unchecked()` will never actually return an
    // error to a running program because any CPI's that fail will halt the entire program.
    crate::program::invoke_unchecked(&instruction, &[]).unwrap();
    let minimum_delegation = get_minimum_delegation_return_data();

    #[cfg(target_arch = "bpf")]
    {
        // SAFETY: The `.unwrap()` is safe because the only way `get_minimum_delegation_return_data()`
        // can fail after doing the CPI is if the stake program is broken.
        minimum_delegation.unwrap()
    }
    #[cfg(not(target_arch = "bpf"))]
    {
        minimum_delegation.unwrap_or_default()
    }
}

/// Helper function for programs to get the return data after calling [`GetMinimumDelegation`]
///
/// This fn handles calling [`get_return_data()`], ensures the result is from the correct
/// program, and returns the correct type.  Returns `None` otherwise.
///
/// [`GetMinimumDelegation`]: super::instruction::StakeInstruction::GetMinimumDelegation
/// [`get_return_data()`]: crate::program::get_return_data
fn get_minimum_delegation_return_data() -> Option<u64> {
    crate::program::get_return_data()
        .and_then(|(program_id, return_data)| {
            (program_id == super::program::id()).then(|| return_data)
        })
        .and_then(|return_data| return_data.try_into().ok())
        .map(u64::from_le_bytes)
}
