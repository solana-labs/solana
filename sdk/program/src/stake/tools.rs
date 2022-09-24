//! Utility functions
use crate::{
    clock::Epoch, program_error::ProgramError, stake::MINIMUM_DELINQUENT_EPOCHS_FOR_DEACTIVATION,
};

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
                .then_some(return_data)
                .ok_or(ProgramError::IncorrectProgramId)
        })
        .and_then(|return_data| {
            return_data
                .try_into()
                .or(Err(ProgramError::InvalidInstructionData))
        })
        .map(u64::from_le_bytes)
}

// Check if the provided `epoch_credits` demonstrate active voting over the previous
// `MINIMUM_DELINQUENT_EPOCHS_FOR_DEACTIVATION`
pub fn acceptable_reference_epoch_credits(
    epoch_credits: &[(Epoch, u64, u64)],
    current_epoch: Epoch,
) -> bool {
    if let Some(epoch_index) = epoch_credits
        .len()
        .checked_sub(MINIMUM_DELINQUENT_EPOCHS_FOR_DEACTIVATION)
    {
        let mut epoch = current_epoch;
        for (vote_epoch, ..) in epoch_credits[epoch_index..].iter().rev() {
            if *vote_epoch != epoch {
                return false;
            }
            epoch = epoch.saturating_sub(1);
        }
        true
    } else {
        false
    }
}

// Check if the provided `epoch_credits` demonstrate delinquency over the previous
// `MINIMUM_DELINQUENT_EPOCHS_FOR_DEACTIVATION`
pub fn eligible_for_deactivate_delinquent(
    epoch_credits: &[(Epoch, u64, u64)],
    current_epoch: Epoch,
) -> bool {
    match epoch_credits.last() {
        None => true,
        Some((epoch, ..)) => {
            if let Some(minimum_epoch) =
                current_epoch.checked_sub(MINIMUM_DELINQUENT_EPOCHS_FOR_DEACTIVATION as Epoch)
            {
                *epoch <= minimum_epoch
            } else {
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_acceptable_reference_epoch_credits() {
        let epoch_credits = [];
        assert!(!acceptable_reference_epoch_credits(&epoch_credits, 0));

        let epoch_credits = [(0, 42, 42), (1, 42, 42), (2, 42, 42), (3, 42, 42)];
        assert!(!acceptable_reference_epoch_credits(&epoch_credits, 3));

        let epoch_credits = [
            (0, 42, 42),
            (1, 42, 42),
            (2, 42, 42),
            (3, 42, 42),
            (4, 42, 42),
        ];
        assert!(!acceptable_reference_epoch_credits(&epoch_credits, 3));
        assert!(acceptable_reference_epoch_credits(&epoch_credits, 4));

        let epoch_credits = [
            (1, 42, 42),
            (2, 42, 42),
            (3, 42, 42),
            (4, 42, 42),
            (5, 42, 42),
        ];
        assert!(acceptable_reference_epoch_credits(&epoch_credits, 5));

        let epoch_credits = [
            (0, 42, 42),
            (2, 42, 42),
            (3, 42, 42),
            (4, 42, 42),
            (5, 42, 42),
        ];
        assert!(!acceptable_reference_epoch_credits(&epoch_credits, 5));
    }

    #[test]
    fn test_eligible_for_deactivate_delinquent() {
        let epoch_credits = [];
        assert!(eligible_for_deactivate_delinquent(&epoch_credits, 42));

        let epoch_credits = [(0, 42, 42)];
        assert!(!eligible_for_deactivate_delinquent(&epoch_credits, 0));

        let epoch_credits = [(0, 42, 42)];
        assert!(!eligible_for_deactivate_delinquent(
            &epoch_credits,
            MINIMUM_DELINQUENT_EPOCHS_FOR_DEACTIVATION as Epoch - 1
        ));
        assert!(eligible_for_deactivate_delinquent(
            &epoch_credits,
            MINIMUM_DELINQUENT_EPOCHS_FOR_DEACTIVATION as Epoch
        ));

        let epoch_credits = [(100, 42, 42)];
        assert!(!eligible_for_deactivate_delinquent(
            &epoch_credits,
            100 + MINIMUM_DELINQUENT_EPOCHS_FOR_DEACTIVATION as Epoch - 1
        ));
        assert!(eligible_for_deactivate_delinquent(
            &epoch_credits,
            100 + MINIMUM_DELINQUENT_EPOCHS_FOR_DEACTIVATION as Epoch
        ));
    }
}
