---
title: Sysvar Cluster Data
---

Solana exposes a variety of cluster state data to programs via
[`sysvar`](terminology.md#sysvar) accounts. These accounts are populated at
known addresses published along with the account layouts in the
[`solana-sdk`](https://docs.rs/solana-sdk/VERSION_FOR_DOCS_RS/solana_sdk/sysvar/index.html),
and outlined below.

To include sysvar data in program operations, pass the sysvar account address in
the list of accounts in a transaction. The account can be read in your
instruction processor like any other account. Access to sysvars is always
*readonly*.

## Clock

The Clock sysvar contains data on cluster time, including the current slot,
epoch, and estimated wall-clock Unix timestamp. It is updated every slot.

- Address: `SysvarC1ock11111111111111111111111111111111`
- Layout: [Clock](https://docs.rs/solana-sdk/VERSION_FOR_DOCS_RS/solana_sdk/sysvar/clock/struct.Clock.html)

## EpochSchedule

The EpochSchedule sysvar contains epoch scheduling constants that are set in
genesis, and enables calculating the number of slots in a given epoch, the epoch
for a given slot, etc. (Note: the epoch schedule is distinct from the
[`leader schedule`](terminology.md#leader-schedule))

- Address: `SysvarEpochSchedu1e111111111111111111111111`
- Layout: [EpochSchedule](https://docs.rs/solana-sdk/VERSION_FOR_DOCS_RS/solana_sdk/sysvar/epoch_schedule/struct.EpochSchedule.html)

## Fees

The Fees sysvar contains the fee calculator for the current slot. It is updated
every slot, based on the fee-rate governor.

- Address: `SysvarFees111111111111111111111111111111111`
- Layout: [Fees](https://docs.rs/solana-sdk/VERSION_FOR_DOCS_RS/solana_sdk/sysvar/fees/struct.Fees.html)

## Instructions

The Instructions sysvar contains the serialized instructions in a Message while
that Message is being processed. This allows program instructions to reference
other instructions in the same transaction. Read more information on
[instruction introspection](implemented-proposals/instruction_introspection.md).

- Address: `Sysvar1nstructions1111111111111111111111111`
- Layout: [Instructions](https://docs.rs/solana-sdk/VERSION_FOR_DOCS_RS/solana_sdk/sysvar/instructions/type.Instructions.html)

## RecentBlockhashes

The RecentBlockhashes sysvar contains the active recent blockhashes as well as
their associated fee calculators. It is updated every slot.

- Address: `SysvarRecentB1ockHashes11111111111111111111`
- Layout: [RecentBlockhashes](https://docs.rs/solana-sdk/VERSION_FOR_DOCS_RS/solana_sdk/sysvar/recent_blockhashes/struct.RecentBlockhashes.html)

## Rent

The Rent sysvar contains the rental rate. Currently, the rate is static and set
in genesis. The Rent burn percentage is modified by manual feature activation.

- Address: `SysvarRent111111111111111111111111111111111`
- Layout: [Rent](https://docs.rs/solana-sdk/VERSION_FOR_DOCS_RS/solana_sdk/sysvar/rent/struct.Rent.html)

## SlotHashes

The SlotHashes sysvar contains the most recent hashes of the slot's parent
banks. It is updated every slot.

- Address: `SysvarS1otHashes111111111111111111111111111`
- Layout: [SlotHashes](https://docs.rs/solana-sdk/VERSION_FOR_DOCS_RS/solana_sdk/sysvar/slot_hashes/struct.SlotHashes.html)

## SlotHistory

The SlotHistory sysvar contains a bitvector of slots present over the last
epoch. It is updated every slot.

- Address: `SysvarS1otHistory11111111111111111111111111`
- Layout: [SlotHistory](https://docs.rs/solana-sdk/VERSION_FOR_DOCS_RS/solana_sdk/sysvar/slot_history/struct.SlotHistory.html)

## StakeHistory

The StakeHistory sysvar contains the history of cluster-wide stake activations
and de-activations per epoch. It is updated at the start of every epoch.

- Address: `SysvarStakeHistory1111111111111111111111111`
- Layout: [StakeHistory](https://docs.rs/solana-sdk/VERSION_FOR_DOCS_RS/solana_sdk/sysvar/stake_history/struct.StakeHistory.html)
