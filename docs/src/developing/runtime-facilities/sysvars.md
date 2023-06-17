---
title: Sysvar Cluster Data
---

Solana exposes a variety of cluster state data to programs via
[`sysvar`](terminology.md#sysvar) accounts. These accounts are populated at
known addresses published along with the account layouts in the
[`solana-program`
crate](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/sysvar/index.html),
and outlined below.

There are two ways for a program to access a sysvar.

The first is to query the sysvar at runtime via the sysvar's `get()` function:

```
let clock = Clock::get()
```

The following sysvars support `get`:
- Clock
- EpochSchedule
- Fees
- Rent
- EpochRewards

The second is to pass the sysvar to the program as an account by including its address as one of the accounts in the `Instruction` and then deserializing the data during execution.  Access to sysvars accounts is
always _readonly_.

```
let clock_sysvar_info = next_account_info(account_info_iter)?;
let clock = Clock::from_account_info(&clock_sysvar_info)?;
```

The first method is more efficient and does not require that the sysvar account be passed to the program, or specified in the `Instruction` the program is processing.

## Clock

The Clock sysvar contains data on cluster time, including the current slot,
epoch, and estimated wall-clock Unix timestamp. It is updated every slot.

- Address: `SysvarC1ock11111111111111111111111111111111`
- Layout: [Clock](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/clock/struct.Clock.html)
- Fields:

  - `slot`: the current slot
  - `epoch_start_timestamp`: the Unix timestamp of the first slot in this epoch. In the first slot of an epoch, this timestamp is identical to the `unix_timestamp` (below).
  - `epoch`: the current epoch
  - `leader_schedule_epoch`: the most recent epoch for which the leader schedule has already been generated
  - `unix_timestamp`: the Unix timestamp of this slot.

  Each slot has an estimated duration based on Proof of History. But in reality,
  slots may elapse faster and slower than this estimate. As a result, the Unix
  timestamp of a slot is generated based on oracle input from voting validators.
  This timestamp is calculated as the stake-weighted median of timestamp
  estimates provided by votes, bounded by the expected time elapsed since the
  start of the epoch.

  More explicitly: for each slot, the most recent vote timestamp provided by
  each validator is used to generate a timestamp estimate for the current slot
  (the elapsed slots since the vote timestamp are assumed to be
  Bank::ns_per_slot). Each timestamp estimate is associated with the stake
  delegated to that vote account to create a distribution of timestamps by
  stake. The median timestamp is used as the `unix_timestamp`, unless the
  elapsed time since the `epoch_start_timestamp` has deviated from the expected
  elapsed time by more than 25%.

## EpochSchedule

The EpochSchedule sysvar contains epoch scheduling constants that are set in
genesis, and enables calculating the number of slots in a given epoch, the epoch
for a given slot, etc. (Note: the epoch schedule is distinct from the [`leader schedule`](terminology.md#leader-schedule))

- Address: `SysvarEpochSchedu1e111111111111111111111111`
- Layout:
  [EpochSchedule](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/epoch_schedule/struct.EpochSchedule.html)

## Fees

The Fees sysvar contains the fee calculator for the current slot. It is updated
every slot, based on the fee-rate governor.

- Address: `SysvarFees111111111111111111111111111111111`
- Layout:
  [Fees](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/sysvar/fees/struct.Fees.html)

## Instructions

The Instructions sysvar contains the serialized instructions in a Message while
that Message is being processed. This allows program instructions to reference
other instructions in the same transaction. Read more information on
[instruction introspection](implemented-proposals/instruction_introspection.md).

- Address: `Sysvar1nstructions1111111111111111111111111`
- Layout:
  [Instructions](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/sysvar/instructions/struct.Instructions.html)

## RecentBlockhashes

The RecentBlockhashes sysvar contains the active recent blockhashes as well as
their associated fee calculators. It is updated every slot. Entries are ordered
by descending block height, so the first entry holds the most recent block hash,
and the last entry holds an old block hash.

- Address: `SysvarRecentB1ockHashes11111111111111111111`
- Layout:
  [RecentBlockhashes](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/sysvar/recent_blockhashes/struct.RecentBlockhashes.html)

## Rent

The Rent sysvar contains the rental rate. Currently, the rate is static and set
in genesis. The Rent burn percentage is modified by manual feature activation.

- Address: `SysvarRent111111111111111111111111111111111`
- Layout:
  [Rent](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/rent/struct.Rent.html)

## SlotHashes

The SlotHashes sysvar contains the most recent hashes of the slot's parent
banks. It is updated every slot.

- Address: `SysvarS1otHashes111111111111111111111111111`
- Layout:
  [SlotHashes](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/slot_hashes/struct.SlotHashes.html)

## SlotHistory

The SlotHistory sysvar contains a bitvector of slots present over the last
epoch. It is updated every slot.

- Address: `SysvarS1otHistory11111111111111111111111111`
- Layout:
  [SlotHistory](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/slot_history/struct.SlotHistory.html)

## StakeHistory

The StakeHistory sysvar contains the history of cluster-wide stake activations
and de-activations per epoch. It is updated at the start of every epoch.

- Address: `SysvarStakeHistory1111111111111111111111111`
- Layout:
  [StakeHistory](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/stake_history/struct.StakeHistory.html)

## EpochRewards

The EpochRewards sysvar tracks the progress of epoch rewards distribution. The
sysvar is created in the first block of the epoch, and lasts for several blocks
while paying out the rewards. When all rewards have been distributed, the sysvar
is deleted. Unlike other sysvars, which almost always exist on-chain,
EpochRewards sysvar only exists during the reward period. Therefore, calling
`EpochRewards::get()` on blocks that are outside of the reward period will
return an error, i.e. `UnsupportedSysvar`. This can serve as a method for
determining whether epoch rewards distribution has finished.

- Address: `SysvarEpochRewards1111111111111111111111111`
- Layout:
  [EpochRewards](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/epoch_rewards/struct.EpochRewards.html)

## LastRestartSlot

The LastRestartSlot sysvar contains the slot number of the last restart or _0_ (zero) if none ever happened.

- Address: `SysvarLastRestartS1ot1111111111111111111111`
- Layout:
  [LastRestartSlot](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/last_restart_slot/struct.LastRestartSlot.html)
