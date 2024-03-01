//! Information about stake and voter rewards based on stake state.
//! Used by `solana-runtime`.

use {
    crate::points::{
        calculate_stake_points_and_credits, CalculatedStakePoints, InflationPointCalculationEvent,
        PointValue, SkippedReason,
    },
    solana_sdk::{
        account::{AccountSharedData, WritableAccount},
        account_utils::StateMut,
        clock::Epoch,
        instruction::InstructionError,
        stake::{
            instruction::StakeError,
            state::{Stake, StakeStateV2},
        },
        stake_history::StakeHistory,
    },
    solana_vote_program::vote_state::VoteState,
};

#[derive(Debug, PartialEq, Eq)]
struct CalculatedStakeRewards {
    staker_rewards: u64,
    voter_rewards: u64,
    new_credits_observed: u64,
}

// utility function, used by runtime
// returns a tuple of (stakers_reward,voters_reward)
#[doc(hidden)]
pub fn redeem_rewards(
    rewarded_epoch: Epoch,
    stake_state: StakeStateV2,
    stake_account: &mut AccountSharedData,
    vote_state: &VoteState,
    point_value: &PointValue,
    stake_history: &StakeHistory,
    inflation_point_calc_tracer: Option<impl Fn(&InflationPointCalculationEvent)>,
    new_rate_activation_epoch: Option<Epoch>,
) -> Result<(u64, u64), InstructionError> {
    if let StakeStateV2::Stake(meta, mut stake, stake_flags) = stake_state {
        if let Some(inflation_point_calc_tracer) = inflation_point_calc_tracer.as_ref() {
            inflation_point_calc_tracer(
                &InflationPointCalculationEvent::EffectiveStakeAtRewardedEpoch(stake.stake(
                    rewarded_epoch,
                    stake_history,
                    new_rate_activation_epoch,
                )),
            );
            inflation_point_calc_tracer(&InflationPointCalculationEvent::RentExemptReserve(
                meta.rent_exempt_reserve,
            ));
            inflation_point_calc_tracer(&InflationPointCalculationEvent::Commission(
                vote_state.commission,
            ));
        }

        if let Some((stakers_reward, voters_reward)) = redeem_stake_rewards(
            rewarded_epoch,
            &mut stake,
            point_value,
            vote_state,
            stake_history,
            inflation_point_calc_tracer,
            new_rate_activation_epoch,
        ) {
            stake_account.checked_add_lamports(stakers_reward)?;
            stake_account.set_state(&StakeStateV2::Stake(meta, stake, stake_flags))?;

            Ok((stakers_reward, voters_reward))
        } else {
            Err(StakeError::NoCreditsToRedeem.into())
        }
    } else {
        Err(InstructionError::InvalidAccountData)
    }
}

fn redeem_stake_rewards(
    rewarded_epoch: Epoch,
    stake: &mut Stake,
    point_value: &PointValue,
    vote_state: &VoteState,
    stake_history: &StakeHistory,
    inflation_point_calc_tracer: Option<impl Fn(&InflationPointCalculationEvent)>,
    new_rate_activation_epoch: Option<Epoch>,
) -> Option<(u64, u64)> {
    if let Some(inflation_point_calc_tracer) = inflation_point_calc_tracer.as_ref() {
        inflation_point_calc_tracer(&InflationPointCalculationEvent::CreditsObserved(
            stake.credits_observed,
            None,
        ));
    }
    calculate_stake_rewards(
        rewarded_epoch,
        stake,
        point_value,
        vote_state,
        stake_history,
        inflation_point_calc_tracer.as_ref(),
        new_rate_activation_epoch,
    )
    .map(|calculated_stake_rewards| {
        if let Some(inflation_point_calc_tracer) = inflation_point_calc_tracer {
            inflation_point_calc_tracer(&InflationPointCalculationEvent::CreditsObserved(
                stake.credits_observed,
                Some(calculated_stake_rewards.new_credits_observed),
            ));
        }
        stake.credits_observed = calculated_stake_rewards.new_credits_observed;
        stake.delegation.stake += calculated_stake_rewards.staker_rewards;
        (
            calculated_stake_rewards.staker_rewards,
            calculated_stake_rewards.voter_rewards,
        )
    })
}

/// for a given stake and vote_state, calculate what distributions and what updates should be made
/// returns a tuple in the case of a payout of:
///   * staker_rewards to be distributed
///   * voter_rewards to be distributed
///   * new value for credits_observed in the stake
/// returns None if there's no payout or if any deserved payout is < 1 lamport
fn calculate_stake_rewards(
    rewarded_epoch: Epoch,
    stake: &Stake,
    point_value: &PointValue,
    vote_state: &VoteState,
    stake_history: &StakeHistory,
    inflation_point_calc_tracer: Option<impl Fn(&InflationPointCalculationEvent)>,
    new_rate_activation_epoch: Option<Epoch>,
) -> Option<CalculatedStakeRewards> {
    // ensure to run to trigger (optional) inflation_point_calc_tracer
    let CalculatedStakePoints {
        points,
        new_credits_observed,
        mut force_credits_update_with_skipped_reward,
    } = calculate_stake_points_and_credits(
        stake,
        vote_state,
        stake_history,
        inflation_point_calc_tracer.as_ref(),
        new_rate_activation_epoch,
    );

    // Drive credits_observed forward unconditionally when rewards are disabled
    // or when this is the stake's activation epoch
    if point_value.rewards == 0 {
        if let Some(inflation_point_calc_tracer) = inflation_point_calc_tracer.as_ref() {
            inflation_point_calc_tracer(&SkippedReason::DisabledInflation.into());
        }
        force_credits_update_with_skipped_reward = true;
    } else if stake.delegation.activation_epoch == rewarded_epoch {
        // not assert!()-ed; but points should be zero
        if let Some(inflation_point_calc_tracer) = inflation_point_calc_tracer.as_ref() {
            inflation_point_calc_tracer(&SkippedReason::JustActivated.into());
        }
        force_credits_update_with_skipped_reward = true;
    }

    if force_credits_update_with_skipped_reward {
        return Some(CalculatedStakeRewards {
            staker_rewards: 0,
            voter_rewards: 0,
            new_credits_observed,
        });
    }

    if points == 0 {
        if let Some(inflation_point_calc_tracer) = inflation_point_calc_tracer.as_ref() {
            inflation_point_calc_tracer(&SkippedReason::ZeroPoints.into());
        }
        return None;
    }
    if point_value.points == 0 {
        if let Some(inflation_point_calc_tracer) = inflation_point_calc_tracer.as_ref() {
            inflation_point_calc_tracer(&SkippedReason::ZeroPointValue.into());
        }
        return None;
    }

    let rewards = points
        .checked_mul(u128::from(point_value.rewards))
        .unwrap()
        .checked_div(point_value.points)
        .unwrap();

    let rewards = u64::try_from(rewards).unwrap();

    // don't bother trying to split if fractional lamports got truncated
    if rewards == 0 {
        if let Some(inflation_point_calc_tracer) = inflation_point_calc_tracer.as_ref() {
            inflation_point_calc_tracer(&SkippedReason::ZeroReward.into());
        }
        return None;
    }
    let (voter_rewards, staker_rewards, is_split) = vote_state.commission_split(rewards);
    if let Some(inflation_point_calc_tracer) = inflation_point_calc_tracer.as_ref() {
        inflation_point_calc_tracer(&InflationPointCalculationEvent::SplitRewards(
            rewards,
            voter_rewards,
            staker_rewards,
            (*point_value).clone(),
        ));
    }

    if (voter_rewards == 0 || staker_rewards == 0) && is_split {
        // don't collect if we lose a whole lamport somewhere
        //  is_split means there should be tokens on both sides,
        //  uncool to move credits_observed if one side didn't get paid
        if let Some(inflation_point_calc_tracer) = inflation_point_calc_tracer.as_ref() {
            inflation_point_calc_tracer(&SkippedReason::TooEarlyUnfairSplit.into());
        }
        return None;
    }

    Some(CalculatedStakeRewards {
        staker_rewards,
        voter_rewards,
        new_credits_observed,
    })
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{points::null_tracer, stake_state::new_stake},
        solana_sdk::{native_token, pubkey::Pubkey},
    };

    #[test]
    fn test_stake_state_redeem_rewards() {
        let mut vote_state = VoteState::default();
        // assume stake.stake() is right
        // bootstrap means fully-vested stake at epoch 0
        let stake_lamports = 1;
        let mut stake = new_stake(
            stake_lamports,
            &Pubkey::default(),
            &vote_state,
            std::u64::MAX,
        );

        // this one can't collect now, credits_observed == vote_state.credits()
        assert_eq!(
            None,
            redeem_stake_rewards(
                0,
                &mut stake,
                &PointValue {
                    rewards: 1_000_000_000,
                    points: 1
                },
                &vote_state,
                &StakeHistory::default(),
                null_tracer(),
                None,
            )
        );

        // put 2 credits in at epoch 0
        vote_state.increment_credits(0, 1);
        vote_state.increment_credits(0, 1);

        // this one should be able to collect exactly 2
        assert_eq!(
            Some((stake_lamports * 2, 0)),
            redeem_stake_rewards(
                0,
                &mut stake,
                &PointValue {
                    rewards: 1,
                    points: 1
                },
                &vote_state,
                &StakeHistory::default(),
                null_tracer(),
                None,
            )
        );

        assert_eq!(
            stake.delegation.stake,
            stake_lamports + (stake_lamports * 2)
        );
        assert_eq!(stake.credits_observed, 2);
    }

    #[test]
    fn test_stake_state_calculate_rewards() {
        let mut vote_state = VoteState::default();
        // assume stake.stake() is right
        // bootstrap means fully-vested stake at epoch 0
        let mut stake = new_stake(1, &Pubkey::default(), &vote_state, std::u64::MAX);

        // this one can't collect now, credits_observed == vote_state.credits()
        assert_eq!(
            None,
            calculate_stake_rewards(
                0,
                &stake,
                &PointValue {
                    rewards: 1_000_000_000,
                    points: 1
                },
                &vote_state,
                &StakeHistory::default(),
                null_tracer(),
                None,
            )
        );

        // put 2 credits in at epoch 0
        vote_state.increment_credits(0, 1);
        vote_state.increment_credits(0, 1);

        // this one should be able to collect exactly 2
        assert_eq!(
            Some(CalculatedStakeRewards {
                staker_rewards: stake.delegation.stake * 2,
                voter_rewards: 0,
                new_credits_observed: 2,
            }),
            calculate_stake_rewards(
                0,
                &stake,
                &PointValue {
                    rewards: 2,
                    points: 2 // all his
                },
                &vote_state,
                &StakeHistory::default(),
                null_tracer(),
                None,
            )
        );

        stake.credits_observed = 1;
        // this one should be able to collect exactly 1 (already observed one)
        assert_eq!(
            Some(CalculatedStakeRewards {
                staker_rewards: stake.delegation.stake,
                voter_rewards: 0,
                new_credits_observed: 2,
            }),
            calculate_stake_rewards(
                0,
                &stake,
                &PointValue {
                    rewards: 1,
                    points: 1
                },
                &vote_state,
                &StakeHistory::default(),
                null_tracer(),
                None,
            )
        );

        // put 1 credit in epoch 1
        vote_state.increment_credits(1, 1);

        stake.credits_observed = 2;
        // this one should be able to collect the one just added
        assert_eq!(
            Some(CalculatedStakeRewards {
                staker_rewards: stake.delegation.stake,
                voter_rewards: 0,
                new_credits_observed: 3,
            }),
            calculate_stake_rewards(
                1,
                &stake,
                &PointValue {
                    rewards: 2,
                    points: 2
                },
                &vote_state,
                &StakeHistory::default(),
                null_tracer(),
                None,
            )
        );

        // put 1 credit in epoch 2
        vote_state.increment_credits(2, 1);
        // this one should be able to collect 2 now
        assert_eq!(
            Some(CalculatedStakeRewards {
                staker_rewards: stake.delegation.stake * 2,
                voter_rewards: 0,
                new_credits_observed: 4,
            }),
            calculate_stake_rewards(
                2,
                &stake,
                &PointValue {
                    rewards: 2,
                    points: 2
                },
                &vote_state,
                &StakeHistory::default(),
                null_tracer(),
                None,
            )
        );

        stake.credits_observed = 0;
        // this one should be able to collect everything from t=0 a warmed up stake of 2
        // (2 credits at stake of 1) + (1 credit at a stake of 2)
        assert_eq!(
            Some(CalculatedStakeRewards {
                staker_rewards: stake.delegation.stake * 2 // epoch 0
                    + stake.delegation.stake // epoch 1
                    + stake.delegation.stake, // epoch 2
                voter_rewards: 0,
                new_credits_observed: 4,
            }),
            calculate_stake_rewards(
                2,
                &stake,
                &PointValue {
                    rewards: 4,
                    points: 4
                },
                &vote_state,
                &StakeHistory::default(),
                null_tracer(),
                None,
            )
        );

        // same as above, but is a really small commission out of 32 bits,
        //  verify that None comes back on small redemptions where no one gets paid
        vote_state.commission = 1;
        assert_eq!(
            None, // would be Some((0, 2 * 1 + 1 * 2, 4)),
            calculate_stake_rewards(
                2,
                &stake,
                &PointValue {
                    rewards: 4,
                    points: 4
                },
                &vote_state,
                &StakeHistory::default(),
                null_tracer(),
                None,
            )
        );
        vote_state.commission = 99;
        assert_eq!(
            None, // would be Some((0, 2 * 1 + 1 * 2, 4)),
            calculate_stake_rewards(
                2,
                &stake,
                &PointValue {
                    rewards: 4,
                    points: 4
                },
                &vote_state,
                &StakeHistory::default(),
                null_tracer(),
                None,
            )
        );

        // now one with inflation disabled. no one gets paid, but we still need
        // to advance the stake state's credits_observed field to prevent back-
        // paying rewards when inflation is turned on.
        assert_eq!(
            Some(CalculatedStakeRewards {
                staker_rewards: 0,
                voter_rewards: 0,
                new_credits_observed: 4,
            }),
            calculate_stake_rewards(
                2,
                &stake,
                &PointValue {
                    rewards: 0,
                    points: 4
                },
                &vote_state,
                &StakeHistory::default(),
                null_tracer(),
                None,
            )
        );

        // credits_observed remains at previous level when vote_state credits are
        // not advancing and inflation is disabled
        stake.credits_observed = 4;
        assert_eq!(
            Some(CalculatedStakeRewards {
                staker_rewards: 0,
                voter_rewards: 0,
                new_credits_observed: 4,
            }),
            calculate_stake_rewards(
                2,
                &stake,
                &PointValue {
                    rewards: 0,
                    points: 4
                },
                &vote_state,
                &StakeHistory::default(),
                null_tracer(),
                None,
            )
        );

        assert_eq!(
            CalculatedStakePoints {
                points: 0,
                new_credits_observed: 4,
                force_credits_update_with_skipped_reward: false,
            },
            calculate_stake_points_and_credits(
                &stake,
                &vote_state,
                &StakeHistory::default(),
                null_tracer(),
                None
            )
        );

        // credits_observed is auto-rewinded when vote_state credits are assumed to have been
        // recreated
        stake.credits_observed = 1000;
        // this is new behavior 1; return the post-recreation rewinded credits from the vote account
        assert_eq!(
            CalculatedStakePoints {
                points: 0,
                new_credits_observed: 4,
                force_credits_update_with_skipped_reward: true,
            },
            calculate_stake_points_and_credits(
                &stake,
                &vote_state,
                &StakeHistory::default(),
                null_tracer(),
                None
            )
        );
        // this is new behavior 2; don't hint when credits both from stake and vote are identical
        stake.credits_observed = 4;
        assert_eq!(
            CalculatedStakePoints {
                points: 0,
                new_credits_observed: 4,
                force_credits_update_with_skipped_reward: false,
            },
            calculate_stake_points_and_credits(
                &stake,
                &vote_state,
                &StakeHistory::default(),
                null_tracer(),
                None
            )
        );

        // get rewards and credits observed when not the activation epoch
        vote_state.commission = 0;
        stake.credits_observed = 3;
        stake.delegation.activation_epoch = 1;
        assert_eq!(
            Some(CalculatedStakeRewards {
                staker_rewards: stake.delegation.stake, // epoch 2
                voter_rewards: 0,
                new_credits_observed: 4,
            }),
            calculate_stake_rewards(
                2,
                &stake,
                &PointValue {
                    rewards: 1,
                    points: 1
                },
                &vote_state,
                &StakeHistory::default(),
                null_tracer(),
                None,
            )
        );

        // credits_observed is moved forward for the stake's activation epoch,
        // and no rewards are perceived
        stake.delegation.activation_epoch = 2;
        stake.credits_observed = 3;
        assert_eq!(
            Some(CalculatedStakeRewards {
                staker_rewards: 0,
                voter_rewards: 0,
                new_credits_observed: 4,
            }),
            calculate_stake_rewards(
                2,
                &stake,
                &PointValue {
                    rewards: 1,
                    points: 1
                },
                &vote_state,
                &StakeHistory::default(),
                null_tracer(),
                None,
            )
        );
    }

    #[test]
    fn test_stake_state_calculate_points_with_typical_values() {
        let vote_state = VoteState::default();

        // bootstrap means fully-vested stake at epoch 0 with
        //  10_000_000 SOL is a big but not unreasaonable stake
        let stake = new_stake(
            native_token::sol_to_lamports(10_000_000f64),
            &Pubkey::default(),
            &vote_state,
            std::u64::MAX,
        );

        // this one can't collect now, credits_observed == vote_state.credits()
        assert_eq!(
            None,
            calculate_stake_rewards(
                0,
                &stake,
                &PointValue {
                    rewards: 1_000_000_000,
                    points: 1
                },
                &vote_state,
                &StakeHistory::default(),
                null_tracer(),
                None,
            )
        );
    }
}
