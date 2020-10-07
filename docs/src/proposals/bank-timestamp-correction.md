---
title: Bank Timestamp Drift Correction
---

## Problem

Currently, each Bank estimates its timestamp as an offset from genesis using a
theoretical slots-per-second constant. This timestamp gets inputted into the
Clock sysvar and is used to assess time-based stake account lockups. However the
actual slots processed per second on the public clusters has been much slower
than the goal, meaning lockups will not actually expire on (or anytime near) the
date the lockup is set to expire.

## Proposed Solution

The [validator timestamp oracle](../implemented-proposals/validator-timestamp-oracle.md)
provides real-world time information, and validators now timestamp every vote.
As a result vote-account state can be used to get the stake-weighted timestamp
of any slot and correct the Bank/clock sysvar drift.

### Correction at Epoch Boundaries

Epoch boundaries seem like the right frequency and time to make this correction,
given the other operations that happen at that time. Then, Clock sysvar
timestamps for following Banks can be estimated from the start of the epoch,
instead of the start of all time, resulting in much less drift.

Specifically, create a new EpochTimestamps sysvar account to store the corrected
timestamp and the epoch start slot; easily feature-gated and ensures the
corrected timestamp will be included in snapshots.

### Sampling

If the corrected timestamp is simply updated to be the stake-weighted timestamp
of the block before the boundary, it provides a single opportunity for a heavily
staked validator or a cabal of validators to control the Bank timestamp and
lockups for the entire following epoch.

To dilute this vulnerability, sample the stake-weighted timestamp at intervals
throughout the epoch, and use the average slot duration between samples to
determine the corrected timestamp.

At each epoch boundary, generate a map of slots in the upcoming epoch from which
to take timestamp samples, also stored in the EpochTimestamps sysvar. Proposed
frequency: approx every 30min; 4500 slots currently. This will currently yield
96 samples.

As those designated slots pass, populate the stake-weighted timestamp in the
sysvar map. At the next epoch boundary, use the populated samples to determine the
corrected timestamp.

The map of slots to sample needs to be generated deterministically to maintain
consensus, so it does still present an opportunity for exploit. One option would
be to use only some of the populated samples to determine the corrected
timestamp; choosing which ones at the end of the epoch based on the last
blockhash. But a malicious actor could still affect the corrected timestamp by
manipulating timestamps for all of the sample slots, or indeed all of the slots
in the epoch. So there doesn't appear to be much benefit to the additional
complexity.

### Correction Math

To determine the corrected timestamp, calculate the average slot duration
between samples and interpolate.
