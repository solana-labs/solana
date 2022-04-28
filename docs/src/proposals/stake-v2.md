---
title: Stake Account Version 2
---

The current stake program is battle-tested, but time has revealed a few important
issues:

1. Redelegations are not possible

Whales can't easily redelegate their positions, since they do not want to lose
out on an epoch of rewards.

2. Rewards calculations at epoch boundaries take too long

Since all of the stake accounts simultaneously receive rewards at the start of an epoch,
the network experiences a big load to process these accounts. This feature
is extremely useful for users, since they don't need to do anything to claim
rewards. On the flip side, it introduces too much load on validators.

With a new stake account, we can address both of these issues.

## New Stake Account

The current `StakeState` is 200 bytes, and can be in 4 states:

* `Uninitialized`
* `Initialized(Meta)`: the rent-exempt reserve, authorized keys, and lockup
* `Stake(Meta, Stake)`: `Delegation` (validator, stake amount, activation / deactivation epochs, warmup rate) + `credits_observed` (used for rewards calc)
* `RewardsPool`: deprecated

To properly perform a redelegation, we need to store at least the
next validator's vote pubkey and a redelegation epoch, which means that
redelegation absolutely requires a larger struct.

### Structure

We need to introduce a new type and an upgrade path from legacy / V1 stakes:

```
struct StakeV2 {
    meta: Meta,
    stake: Stake,
    last_claim_epoch: Epoch, // needed to know if it's up-to-date, see `ClaimRewards`
    redelegation: Redelegation,
}
struct Redelegation {
    voter_pubkey: Pubkey,
    redelegation_epoch: Epoch,
    credits_observed: u64,
}
```

With this information, we can calculate multi-epoch redelegations when the cluster
sees more than 25% stake movement.

### Interoperability with V1

Since loads of places use `mem::size_of::<StakeState>()`, and we still need
to support that, that struct won't get touched. On the flip side, programs
need to process both. Since there's already a neat interface on `StakeState`,
we can do something similar with a new type:

```
enum StakeStateV2 {
    Uninitialized,
    Initialized(Meta),
    StakeV1(Meta, Stake),
    RewardsPool,
    StakeV2(StakeV2),
}
impl StakeStateV2 {
    /// ... all the same as StakeState ...
    pub fn redelegation(&self) -> Option<Redelegation> {
      match self {
          Self::StakeV2(stake) => Some(stake.redelegation),
          _ => None,
      }
    }
}
```

Programs update to use `StakeStateV2` everywhere, and existing code still works.
When an `Initialized` stake is delegated, the stake program makes it a `StakeV1`
if the size is `200`, and a `StakeV2` if the size is `256`.

### Epoch Boundary Calculations

While we're at it, new stake accounts can also solve point number 2, by having
their rewards paid through an update instruction, and not automatically at every
epoch boundary.

During epoch boundary, the validator vote accounts receive all stake V2
account rewards as withheld lamports in their account. They cannot
touch these, and stake accounts may claim them.

The biggest users of stake accounts, ie stake pools and stake bots, already
touch stake accounts every epoch during their bookkeeping, so this adds trivial
work for them.

Legacy stake accounts are unaffected, and inflation continues to pay out their rewards
at epoch boundaries automatically.

## New Stake Program Instructions

`Split` / `Merge` / `Deactivate` are only allowed if the stake account has
been updated this epoch, or after deactivation epoch if applicable.

Side note: deactivate might actually be OK, since we could technically calculate
the rewards even after deactivation.

### UpgradeToV2

Given a V1 stake account, a signature from the stake or withdraw authority, and
a funding account, reallocate the existing stake account and convert it to a V2
stake. The funding account adds the lamports necessary to cover the new rent-exemption.

Side note: this would also be a great time to include the rent-exemption in the
delegation. That way, smaller stakers get a tiny bit more, and there's a couple of other
nice side effects. Ask me about it.

### Redelegate

Given a V2 stake account, vote account, and stake authority signature, change the
redelegation. Follows most of the same rules as delegate.

If it's going through a multi-epoch redelegation, be sure that previous epoch
rewards have been claimed, and update the vote pubkey / credits / epoch.

### (Also in the Vote Program) ClaimRewards

Given only a V2 stake account and the delegated vote account (or two vote accounts
during multi-epoch redelegation), calculate the rewards gained since the last
claim, and withdraw the lamports from the vote account.

This instruction is permissionless.

For easier use with existing tools, if a redelegation is complete, move the
redelegation information over into the main delegation.

Implementation note: claiming lamports from the vote account and also updating the
stake account is tricky since only the owning program can deduct lamports or modify
data, and we need to do both conditionally.

Potential solution: the vote program CPIs into the stake program to update the
credits observed, then transfers the lamports from the vote account to the stake account.
The stake program must enforce that only the vote program can call it, otherwise
it's possible to deprive an account of their rewards.

BIG ISSUE TO CONSIDER: vote accounts only store 64 epochs of credits, which means
that we can only accurately calculate 64 epochs of credits.

Potential solution: if it's been too long, drop rewards. This is easiest, but not great.
We can include a mechanism where it's possible to claim a tiny portion of rewards from
an account if the update is done during the 64th epoch since the last claim.

This slice of the rewards could be claimable by anyone, or only the authorized
voter on the account.

## New Vote Account Field

Vote accounts need to keep track of `withheld_rewards`. We should be able to steal
some bytes from `prior_voters`, since we only need a `u64` for the withheld amount.

`Withdraw` needs to respect the withheld amount and rent-exemption. This means
that vote accounts cannot be deleted if any withheld rewards remain. Thankfully,
since claiming rewards is permissionless, a node operator can do this themselves
and then delete their vote account.

Validators may get confused about the lamports in their account, so improve the
explorer / tools to show how much can be withdrawn, ie
`total_lamports - rent_exemption - withheld_rewards`.

Side note: while we're making changes to the struct, we can also widen `commission`
to a `u16` and finally let validators charge 6.9%, see https://degendaoo.academy/dics
for more context.

## Runtime changes

V2 stakes are not included in the stakes cache, but their delegation amounts are
added to the vote accounts. Every call to the stake program will still hit the
cache, but there won't be an entry to process at epoch boundary.

Since the withheld rewards are included in the vote account, it will always have
a totally accurate delegation amount, regardless of how much is claimed.

Also, since inflation still creates all new lamports at epoch boundary,
the runtime rules on lamport creation are never broken.

## Downstream effects

### Stake Pools

Stake pools and bots can keep using V1 stake accounts no problem in this model.

Since V2 stakes have redelegation, all pools have a huge incentive to upgrade.
Currently, during rebalancing, the moved amount loses one epoch of rewards,
which is a problem for stake pool performance.

Also, since stake pools typically comprise many smaller stakers, and perform
withdrawals through splitting stake accounts, they can keep servicing smaller
holders while maximizing the staked amount in the pool.

### Bots

Instead of staking to and from an inactive reserve stake account, bots can rebalance
by doing a split, redelegating, waiting until activation, and then merging. 

Funds that need high performance can update bots to do this fairly easily.

### Minimum delegation

Since V2 stakes are not included in the stakes cache and are not computed at
epoch boundaries, smaller holders can use them or stake pools.

At the same time, V1 stakes can impose a minimum limit as high as needed.
