//! Utility functions

/// Helper function for programs to call [`GetMinimumDelegation`] and then fetch the return data
///
/// This fn handles performing the CPI to call the [`GetMinimumDelegation`] function, and then
/// calls [`get_minimum_delegation_return_data()`] to fetch the return data.
///
/// [`GetMinimumDelegation`]: super::instruction::StakeInstruction::GetMinimumDelegation
pub fn get_minimum_delegation() -> Option<u64> {
    let instruction = super::instruction::get_minimum_delegation();
    crate::program::invoke(&instruction, &[]).ok()?;
    get_minimum_delegation_return_data()
}

/// Helper function for programs to get the actual data after calling [`GetMinimumDelegation`]
///
/// This fn handles calling [`get_return_data()`], ensures the result is from the correct
/// program, and returns the correct type.  Returns `None` otherwise.
///
/// [`GetMinimumDelegation`]: super::instruction::StakeInstruction::GetMinimumDelegation
/// [`get_return_data()`]: crate::program::get_return_data
pub fn get_minimum_delegation_return_data() -> Option<u64> {
    crate::program::get_return_data()
        .and_then(|(program_id, return_data)| {
            (program_id == super::program::id()).then(|| return_data)
        })
        .and_then(|return_data| return_data.try_into().ok())
        .map(u64::from_le_bytes)
}
