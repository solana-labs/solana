//! Utility functions
use crate::program_error::ProgramError;

/// Helper function for programs to call [`GetMinimumDelegation`] and then fetch the return data
///
/// This fn handles performing the CPI to call the [`GetMinimumDelegation`] function, and then
/// calls [`get_return_data()`] to fetch the return data.
///
/// [`GetMinimumDelegation`]: super::instruction::StakeInstruction::GetMinimumDelegation
/// [`get_return_data()`]: crate::program::get_return_data
pub fn get_minimum_delegation() -> Result<u64, ProgramError> {
    let instruction = super::instruction::get_minimum_delegation();
    crate::program::invoke_unchecked(&instruction, &[])?;
    get_minimum_delegation_return_data()
}

/// Helper function for programs to get the return data after calling [`GetMinimumDelegation`]
///
/// This fn handles calling [`get_return_data()`], ensures the result is from the correct
/// program, and returns the correct type.
///
/// [`GetMinimumDelegation`]: super::instruction::StakeInstruction::GetMinimumDelegation
/// [`get_return_data()`]: crate::program::get_return_data
fn get_minimum_delegation_return_data() -> Result<u64, ProgramError> {
    crate::program::get_return_data()
        .ok_or(ProgramError::InvalidInstructionData)
        .and_then(|(program_id, return_data)| {
            (program_id == super::program::id())
                .then(|| return_data)
                .ok_or(ProgramError::IncorrectProgramId)
        })
        .and_then(|return_data| {
            return_data
                .try_into()
                .or(Err(ProgramError::InvalidInstructionData))
        })
        .map(u64::from_le_bytes)
}
