---
title: Bank Timestamp Correction
---

Each Bank has a timestamp that is stashed in the Clock sysvar and used to assess
time-based stake account lockups. However, since genesis, this value has been
based on a theoretical slots-per-second instead of reality, so it's quite
inaccurate. This poses a problem for lockups, since the accounts will not
register as lockup-free on (or anytime near) the date the lockup is set to
expire.

Block times are already being estimated to cache in Blockstore and long-term
storage using a [validator timestamp oracle](validator-timestamp-oracle.md);
this data provides an opportunity to align the bank timestamp more closely with
real-world time.

The general outline of the proposed implementation is as follows:

- Correct each Bank timestamp using the validator-provided timestamp.
- Update the validator-provided timestamp calculation to use a stake-weighted
  median, rather than a stake-weighted mean.
- Bound the timestamp correction so that it cannot deviate too far from the
  expected theoretical estimate

## Timestamp Correction

On every new Bank, the runtime calculates a realistic timestamp estimate using
validator timestamp-oracle data. The Bank timestamp is corrected to this value
if it is greater than or equal to the previous Bank's timestamp. That is, time
should not ever go backward, so that locked up accounts may be released by the
correction, but once released, accounts can never be relocked by a time
correction.

### Calculating Stake-Weighted Median Timestamp

In order to calculate the estimated timestamp for a particular Bank, the runtime
first needs to get the most recent vote timestamps from the active validator
set. The `Bank::vote_accounts()` method provides the vote accounts state, and
these can be filtered to all accounts whose most recent timestamp was provided
within the last epoch.

From each vote timestamp, an estimate for the current Bank is calculated using
the epoch's target ns_per_slot for any delta between the Bank slot and the
timestamp slot. Each timestamp estimate is associated with the stake delegated
to that vote account, and all the timestamps are collected to create a
stake-weighted timestamp distribution.

From this set, the stake-weighted median timestamp -- that is, the timestamp at
which 50% of the stake estimates a greater-or-equal timestamp and 50% of the
stake estimates a lesser-or-equal timestamp -- is selected as the potential
corrected timestamp.

This stake-weighted median timestamp is preferred over the stake-weighted mean
because the multiplication of stake by proposed timestamp in the mean
calculation allows a node with very small stake to still have a large effect on
the resulting timestamp by proposing a timestamp that is very large or very
small. For example, using the previous `calculate_stake_weighted_timestamp()`
method, a node with 0.00003% of the stake proposing a timestamp of `i64::MAX`
can shift the timestamp forward 97k years!

### Bounding Timestamps

In addition to preventing time moving backward, we can prevent malicious
activity by bounding the corrected timestamp to an acceptable level of deviation
from the theoretical expected time.

This proposal suggests that each timestamp be allowed to deviate up to 25% from
the expected time since the start of the epoch.

In order to calculate the timestamp deviation, each Bank needs to log the
`epoch_start_timestamp` in the Clock sysvar. This value is set to the
`Clock::unix_timestamp` on the first slot of each epoch.

Then, the runtime compares the expected elapsed time since the start of the
epoch with the proposed elapsed time based on the corrected timestamp. If the
corrected elapsed time is within +/- 25% of expected, the corrected timestamp is
accepted. Otherwise, it is bounded to the acceptable deviation.
