---
title: Epoch Accounts Hash
---

*Paraphrasing from https://github.com/solana-labs/solana/issues/26847*

## Background

Rent collection checks every account at least once per epoch.  At each slot, a
deterministic set of accounts (based on the pubkey range) is loaded, checked
for rent collection, and stored back to the Accounts DB at this new slot.

Accounts are stored (rewritten) _even if_ they are unchanged.  This has a few
positive effects.
  1. Once an account is rewritten, the previous version at an older slot is now
     dead. As entire slots and AppendVecs become full of only dead accounts,
     they can then be dropped/recycled.
  2. Each account rewritten due to rent collection is included in that slot's
     bank hash.  Since the bank hash is part of what is voted on for consensus,
     this means every account is verified by the network at least once per
     epoch.

However, there is a big downside to rewriting unchanged accounts: performance.
Storing accounts can be very expensive.  And since accounts now are required to
be rent-exempt, the majority of accounts rewritten due to rent collection are
unchanged.  What if unchanged accounts were no longer rewritten? This would
minimally be a big performance win.


## Problem

If rent collection no longer is rewriting unchanged accounts, we lose the two
positive effects.  Dealing with Positive Effect 1 (from above) will be handled
by _Ancient AppendVecs_, and will not be discussed here.  So how do we still
get the security from Positive Effect 2?  How can we still verify every account
at least once per epoch, *as part of consensus*, but without rewriting
accounts?


## Proposed Solution

Perform a full accounts hash calculation once per epoch, and hash the result
into a bank's `hash`.  This will be known as the _Epoch Accounts Hash_, or
_EAH_.

This retains the Positive Effect 2 from rent collection by checking every
account at least once per epoch.  Thus, any validators with missing, corrupt,
or extra accounts will identify those issues within 1-2 epochs.


### Implementation

Performing a full accounts hash takes a relatively long time to complete.  For
this reason, the EAH calculation must take place in the background.

In order for all the validators to calculate the same accounts hash, the
calculation must be based on the same view of all accounts.  This means the EAH
must be based on a predetermined slot.  This will be known as the `start slot`.
The `start slot` is calculated as an offset into the epoch.  This offset will
be known as the `start offset`.  Formally, the `start slot` is the first root
*greater-than-or-equal-to* `first slot in epoch + start offset`.

Similarly, all the validators must save the EAH into a bank at a predetermined
slot, and offset from the first slot of an epoch.  This will be known as the
`stop slot` and `stop offset`, respectively.

* The `start offset` will be set at one-quarter into the epoch.
* The `stop offset` will be set at three-quarters into the epoch.
* For epochs with 432,000 slots, the `start offset` will be 108,000 and the
  `stop offset` will be 324,000.

These constants may be changed in the future, or may be determined at runtime.
The main justifications for these values are:
1. Do not start the EAH calculation at the beginning of an epoch, as the
   beginning of an epoch is already a time of contention and stress.  There is
   no reason to make this worse.
2. The bank to save the EAH into—the `stop offset`—should be sufficiently far
   in the future to guarantee all validators are able to complete the accounts
   hash calculation in time.
3. The `start offset` should be *after* the `rewarding interval`
   (from [Partitioned Inflationary Rewards Distribution](https://github.com/solana-labs/solana/pull/27455)).
   This ensures stake rewards have been distributed and stored into the
   accounts for this epoch.

Once the EAH calculation is complete, it must be saved somewhere.  Since this
occurs in the background, there is not an associated `Bank` that would make
sense to save into.  Instead, a new field will be added to `AccountsDb` that
will store the EAH.  Later, the bank at slot `stop slot`†¹ will read the EAH from
`AccountsDb` and hash it into its own hash (aka _bank hash_).

EAH calculation will use the existing _accounts background services_ (_ABS_) to
perform the actual calculation.  Requests for EAH calculation will be sent from
`bank_forks::set_root()`†², with a new request type to distinguish an EAH request
from a Snapshot request.  Since the EAH will be part of consensus, it is not
optional; EAH requests will have the highest priority in ABS, and will be
processed first/instead of other requests.

†¹: More precisely, all banks where `bank slot >= stop slot` and `parent slot <
    stop slot` will include the EAH in their _bank hash_.  This ensures EAH
    handles forking around `stop slot`, since only one of these banks will end
    up rooted.

†²: An EAH calculation will be requested when `root bank slot >= start slot`
    and `root parent slot < start slot`.  This handles the scenario where
    validators call `bank_forks::set_root()` at different intervals.


#### Details

#### Snapshots

A snapshot contains all the state necessary to reconstruct the cluster as of a
certain slot.  A snapshot may then need to contain the EAH so that the `stop
slot` can include it.  Consider the following scenarios within an epoch where a
snapshot is requested for slot `X`:


##### 1. `X >= first slot in epoch` and `X < start slot`

Since the `start slot` has not been reached yet, there is nothing special to do
in order to take a snapshot in this scenario.


##### 2. `X == start slot`

The EAH *must* be included in the snapshot.  Since the snapshot process always
calculates the accounts hash, no additional calculations are required.  The
accounts hash calculation result will be used both to store in the snapshot as
the EAH, and for the snapshot hash (which is used at load-time for verification).


##### 3. `X > start slot` and `X < stop slot`

If a snapshot is requested to be created *after* the `start slot` but *before*
the EAH calculation has completed, then it will be impossible to create a
snapshot with the correct EAH.  The snapshot process will wait until the EAH
calculation has completed before proceeding.


##### 4. `X == stop slot`

The EAH has been calculated for this epoch, and has been included in the `stop
slot` bank.  No further handling is required; the snapshot does not need to
contain the EAH.


##### 5. `X > stop slot` and `X <= last slot in epoch`

Same as (4).


#### Corner Cases

#### Minimum Slots per Epoch

An EAH is requested by `BankForks::set_root()`, which happens while setting
*roots*.  The EAH is stored into `Bank`s when they are *frozen*.  Banks are
frozen 32 slots before they are rooted.  For the expected behavior, the EAH
start slot really should be 32 slots before the stop slot. If the number of
slots per epoch is small, this can result in surprising behavior.

Example 1: Assume there are 64 slots per epoch.  The EAH start offset is 16
and the EAH stop offset is 48.  The difference is 32.  So when Bank 48 is
frozen before Bank 16 is rooted, a new EAH request has not yet been requested;
the EAH from the previous epoch is still valid and will be used by Bank 48.

Example 2: Assume there are 66 slots per epoch, then the EAH start offset is
still 16 and the EAH stop offset is now 49.  The difference is now 33.  When
Bank 49 is frozen, Bank 16 will already have been rooted, and thus sent an EAH
request; Bank 49 will wait for the new EAH calculation to complete.

Example 3: Assume there are 32 slots per epoch (the minimum allowed).  The EAH
start offset is 8, and the EAH stop offset is 24.  Similar to Example 1, Bank
24 is frozen around when Bank 24 *of the previous epoch* is rooted.  This
ensures that when the EAH is stored, it'll be for the previous epoch.

In these examples the observed behavior of the EAH is different than when using
the normal 432,000 slots per epoch.  The EAH is still valid and correct with a
small number of slots per epoch; it now has a delay of one epoch.  Since the
epochs themselves can be much faster, security is not reduced.


#### Warping

Warping introduces corner cases into EAH because many slots may be skipped,
including the entire range of `start slot` to `stop slot`.

When warping from before `stop slot` to after, the new bank will include the
existing EAH in its hash during `freeze()`.  If the bank's parent is from
before `start slot`, then a new EAH calculation will not have been requested.
This is safe because warping cannot be used on a live cluster; only for a new
cluster or tests/debugging.  This means _when_ the EAH was calculated is not
germane.

When warping from before `start slot` to after, an EAH calculation will be
requested the next time `set_root()` is called.  Therefore the EAH will be
based on this new bank.  This is also safe and correct.

For specific examples, refer to Appendix A.


#### Implementation Alternatives

##### Perform the EAH calculation in the foreground

The accounts hash calculation takes around 15 seconds (median, on Mainnet-Beta
today).  This is far beyond the slot time; this would be bad UX, and also
decrease network stability.


##### Remove `stop offset`

Instead of having two offsets—one for `start` and one for `stop`—use a single
offset for both.  This delays when the EAH is saved into a Bank and voted on.
The saved EAH is now the EAH from the previous epoch.  This could work; would
reduce the number of "special" slots from two to one.  No significant
advantages observed.


##### Send EAH requests when making a new bank, instead of a new root

When a bank is created, we don't yet know if it will be finalized until it is
rooted, which could result in multiple EAH requests due to forking.  This would
be bad for performance.


### Appendix A: All Warping Scenarios

To enumerate how EAH interacts with warping, refer to the following diagram for
the scenarios below:

```text
  +---------+-----------------+-----------+---------+-----------------+-----------+
  |         >                 <           |         >                 <           |
  |    A    >     B           <     C     |    D    >      E          <     F     |
  |         >                 <           |         >                 <           |
  +---------+-----------------+-----------+---------+-----------------+-----------+
  |         |                 |           |         |                 |           |
  v         v                 v           v         v                 v           v
  epoch 1   start slot 1      stop slot 1 epoch 2   start slot 2      stop slot 2 epoch 3
```


#### parent slot: `A`, warp slot: `A`

No slots important to the EAH have been skipped, so no change in behavior.


#### parent slot: `A`, warp slot: `B`

An EAH calculation will be requested at the warp slot, and then will be
included in the Bank at `slot slot 1`; behavior is unchanged.


#### parent slot: `A`, warp slot: `C` or `D`

The entire EAH range has been skipped; no new EAH calculation will have been
requested for epoch 1.  The warp slot will include the EAH from `epoch 0`.
This is different from the normal behavior.


#### parent slot: `A`, warp slot: `E`

Similar to `A -> B`, an EAH calculation will be requested at the warp slot, and
then will be included in the Bank at `stop slot 2`.  Behavior appears normal.


#### parent slot: `A`, warp slot: `F`

Similar to `A -> C`, no new EAH calculation will be requested.  The warp slot
will include the EAH from `epoch 0`.  This is different from the normal
behavior.


#### parent slot: `B`, warp slot: `B`

Similar to `A -> A`, no slots important to the EAH have been skipped, so no
change in behavior.


#### parent slot: `B`, warp slot: `C` or `D`

This will be observed as normal behavior; the warp slot will include the EAH
that was calculated based on `start slot 1`.


#### parent slot: `B`, warp slot: `E`

Similar to `A -> B`, an EAH calculation will be requested at the warp slot, and
then will be included in the Bank at `stop slot 2`.  Behavior appears normal.


#### parent slot: `B`, warp slot: `F`

The warp slot will include the EAH from `start slot 1`.  Behavior appears
different.
