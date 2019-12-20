# Validator Timestamp Oracle

Third-party users of Solana sometimes need to know the real-world time a block
was produced, generally to meet compliance requirements for external auditors or
law enforcement. This proposal describes a validator timestamp oracle that
would allow a Solana cluster to satisfy this need.

The general outline of the proposed implementation is as follows:

- At regular intervals, each validator records its observed time for a known slot
  on-chain (via a Timestamp added to a slot Vote)
- A client can request a block time for a rooted block using the `getBlockTime`
RPC method. When a client requests a timestamp for block N:

  1. A validator determines a "cluster" timestamp for a recent timestamped slot
  before block N by observing all the timestamped Vote instructions recorded on
  the ledger that reference that slot, and determining the stake-weighted mean
  timestamp.

  2. This recent mean timestamp is then used to calculate the timestamp of
  block N using the cluster's established slot duration

Requirements:
- Any validator replaying the ledger in the future must come up with the same
  time for every block since genesis
- Estimated block times should not drift more than an hour or so before resolving
  to real-world (oracle) data
- The block times are not controlled by a single centralized oracle, but
  ideally based on a function that uses inputs from all validators
- Each validator must maintain a timestamp oracle

The same implementation can provide a timestamp estimate for a not-yet-rooted
block. However, because the most recent timestamped slot may or may not be
rooted yet, this timestamp would be unstable (potentially failing requirement
1). Initial implementation will target rooted blocks, but if there is a use case
for recent-block timestamping, it will be trivial to add the RPC apis in the
future.

## Recording Time

At regular intervals as it is voting on a particular slot, each validator
records its observed time by including a timestamp in its Vote instruction
submission. The corresponding slot for the timestamp is the newest Slot in the
Vote vector (`Vote::slots.iter().max()`). It is signed by the validator's
identity keypair as a usual Vote. In order to enable this reporting, the Vote
struct needs to be extended to include a timestamp field, `timestamp:
Option<UnixTimestamp>`, which will be set to `None` in most Votes.

This proposal suggests that Vote instructions with `Some(timestamp)` be issued
every 30min, which should be short enough to prevent block times drifting very
much, without adding too much transaction overhead to the cluster. Validators
can convert this time to a slot interval using the `slots_per_year` value that
is stored in each bank.

```text
let seconds_in_30min = 1800;
let timestamp_interval = (slots_per_year / SECONDS_PER_YEAR) * seconds_in_30min;
```

Votes with `Some(timestamp)` should be triggered in `replay_stage::handle_votable_bank()`
when `bank.slot() % timestamp_interval == 0`.

### Vote Accounts

A validator's vote account will hold its most recent slot-timestamp in VoteState.

### Vote Program

The on-chain Vote program needs to be extended to process a timestamp sent with
a Vote instruction from validators. In addition to its current process\_vote
functionality (including loading the correct Vote account and verifying that the
transaction signer is the expected validator), this process needs to compare the
timestamp and corresponding slot to the currently stored values to verify that
they are both monotonically increasing, and store the new slot and timestamp in
the account.

## Calculating Stake-Weighted Mean Timestamp

In order to calculate the estimated timestamp for a particular block, a
validator first needs to identify the most recently timestamped slot:

```text
let timestamp_slot = floor(current_slot / timestamp_interval);
```

Then the validator needs to gather all Vote WithTimestamp transactions from the
ledger that reference that slot, using `Blocktree::get_slot_entries()`. As these
transactions could have taken some time to reach and be processed by the leader,
the validator needs to scan several completed blocks after the timestamp\_slot to
get a reasonable set of Timestamps. The exact number of slots will need to be
tuned: More slots will enable greater cluster participation and more timestamp
datapoints; fewer slots will speed how long timestamp filtering takes.

From this collection of transactions, the validator calculates the
stake-weighted mean timestamp, cross-referencing the epoch stakes from
`staking_utils::staked_nodes_at_epoch()`.

Any validator replaying the ledger should derive the same stake-weighted mean
timestamp by processing the Timestamp transactions from the same number of
slots.

## Calculating Estimated Time for a Particular Block

Once the mean timestamp for a known slot is calculated, it is trivial to
calculate the estimated timestamp for subsequent block N:

```text
let block_n_timestamp = mean_timestamp + (block_n_slot_offset * slot_duration);
```

where `block_n_slot_offset` is the difference between the slot of block N and
the timestamp\_slot, and `slot_duration` is derived from the cluster's
`slots_per_year` stored in each Bank
