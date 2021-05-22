---
title: Warp Stake
---

This document proposes a design for warping a fully-activated `Stake` from one validator to another at the end of the current epoch. A major question lies in how to handle per-epoch warp rate (similar to warmup/cooldown).

## Motivation
Currently, in order to delegate activated stake to a different validator, one must unstake, wait at least 1 epoch, and then restake. This leads to missed rewards and hassle.

Rather, warp stake would avoid this.

This promotes informational efficiency in network-wide stake allocation, allowing stake pools and users to respond more quickly and with reduced opportunity cost to new information.

### Design 1: Maintain a List of Warp Requests
In this design, we enforce that every `VoteAccount` transfer must complete in a single epoch, while respecting the warp rate.

There are two ways to achieve this:
1. **Time priority** (first-come-first-serve). Preferred direction.
2. Stake-weighted random (hard to do as this is an urn problem without replacement)

We assume **time-priority** is chosen for the rest of the design proposal.


Concretely, maintain an ordered list of requests. 

`type TransferRequests = Vec<(PubKey, Pubkey)> // stake_account -> new_vote_account`

We also maintain a `HashSet` to dedup stake account pubkeys.

*Eligibility at epoch boundary:* At the turn of the next epoch, the stake account requesting transfer must be `StakeState::Stake` and have all stake activated  i.e. `stake_activating_and_deactivating` reads `(x, 0, 0)`. So we allow stake to still be activating in epoch prior, but not deactivating. When processing, we remove account from the list if failing to meet criteria. 

Pseudocode for how `Bank` processes warp transfers after calling `pay_validator_rewards`:
```
// retain only fully active
trf_req.retain(|(s, _)| get_account(stake_pk).delegation.stake_fully_active());

// transfer accounts up to warp rate
let mut stakes_proccessed = 0;
let mut accounts_moved = 0
for (stake_pk, vote_pk) in trf_req {
    let stake_state = get_account(stake_pk);
    let stake = stake_state.delegation.stake;
    
    // If we will exceed the warp rate for this epoch, we stop further warps
    if stake_moved + stake >= cfg.warp_rate * cluster_stake.effective {
        break;
    }
    stake_state.delegation.vote_pubkey = vote_pk;
    accounts_moved += 1;
    stake_moved += stake;
}

// retain unprocessed but eligible warps for next epoch
trf_req = trf_req.drain(0..accounts_moved).collect();
```

Requests in the waitlist that fail to warp, but did not fail criterion of being fully activated, would retain their positions in the list with highest warp priority for the following epoch.

You can always cancel a transfer at any time before the epoch boundary. Re-requesting a transfer will put you at the end of the request list, however.

**Advantages**:
1. Simple design. Clear behaviour. Avoid struct modifications.

**Disadvantages:**
1. Vector lookup overhead. But #stake accounts is relatively small.
2. (mostly negated) A malicious user with a lot of stake can request for big transfers to fill up warp rate for the epoch, provoking users to try to deactivate instead of warping. However unfulfilled warps have highest priority in next epoch, so warping in the waitlist is probably always better than deactivating. Thus the scare tactic is not effective unless `total_warp_request stake > 2 * warp_rate * effective_stake`, which is a pretty high bar if `warp_rate = 0.25`, for instance.

### Design 2: Wrap/Unwrap from new Struct `TransferableStake`
We define a new `StakeState::TransferStake(meta, TransferableStake)`.

Pseudocode:
```
struct TransferableStake {
    current_stake: Stake,
    transfer_epoch: Epoch,
    new_voter: &PubKey,
}

impl TransferableStake {
    fn transferred(target_epoch: Epoch, clock: Clock) -> u64 {
        ..
        let mut transferred = 0;
        while current_epoch < target_epoch {
            current_epoch += 1;
            // check transferable_in_epoch based on 
            // cluster_transfer_history.transfer, 
            // cluster_stake_history.effective

            transferred += transferable_in_epoch;
        }
        transferred
    }
}

...

impl StakeState {
    fn deactivate/delegate/merge/split() {
        // if TransferStake return error
    }
    
    // Wrap Stake as TransferableStake
    fn wrap_transferable() {
        // if stake.delegation.stake != stake.delegation.stake(), 
        // i.e. not fully activated, return error.
    }

    
    /// allow unwrapping only if not transferred/fully transferred
    fn unwrap_transferable() {
        if let Some(StakeState::TransferStake(meta, t_stake)) = self.state()? {
            if t_stake.transferred() == 0 {
                self.set_state(t_stake.current_stake)?;
            }
            if t_stake.transferred() == t_stake.current_stake.stake {
                let mut stake = t_stake.current_stake;
                stake.delegation.voter_pubkey = t.new_voter;
                self.set_state(stake)?;
            }
        } else {
            // error
        }
        
    } ...
}

// Complicated plumbing
fn redeem_rewards() {
    // If stake_account.state()? = TransferStake 
    
        t_stake.calculate_points_and_credits() .. {
            let stake1 = t_stake.current_stake.delegation.stake() - t_stake.transferred();
            let stake2 = t_stake.transferred();
        }
        t_stake.calculate_rewards()
    }
}

// in: runtime/stakes
struct Stakes {
    delegations: .. // we don't count TransferStakes here...
    transferring_delegations: .. // use this index to walk TransferStake rewards
}
```

**Disadvantages:**
1. Enum and Struct changes would have downstream repercussions. In particular, `rent_exempt_reserve` would change for all `StakeState`, and thus cause massive churn.
2. Calculating rewards becomes fairly complex.

## Tradeoffs and Discussion
Although design 1 has a different way of handling warp rate than with activation/deactivation, it has reasonable behaviour in both times of non-congestion and congestion.

1. **In times of non-congestion**: as long as you request early enough, you always know you will be warped. 
2. **In times of congestion**: If you fail to warp this epoch, you would be automatically queued to highest priority to warp the next epoch. 
    Nonetheless, if you are not guaranteed a warp, although this is the poorer decision in most cases, you can always cancel your request and go for deactivation instead. 

In design 1, you will always know that you can never be stuck warping over multiple epochs. 

In design 2 as in activating/deactivating, there is always a non-zero chance of being stuck warping over multiple epochs, which cannot be pre-determined at the epoch boundary.
