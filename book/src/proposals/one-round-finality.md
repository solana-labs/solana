# Practical Finality in One Round of Consensus

The cluster can achieve a high degree of finality though Tower
consensus as the cluster commits to an exponentially increasing of
safety for a block.  This safety is based on the commitment of any
voting validator from switching votes to an alternative fork.  It
is possible to achieve finality a different way, leaders can agree
not to propose alternative forks.

If only a part of the cluster has voted, then the set of all the
leaders that haven't voted and the leader schedule can be used to
simulate how lockout will grow in the future.  This calculation is
based on the assumption that the partition is static, and that the
same leaders will continue voting together.

## Overview

Leaders commit to producing a block that chains to the leaders last
vote. Breaking this commitment is slashable.  The leader is free
to switch forks and vote for an alternative chain.

Once a block has been voted on, the subsequent leader schedule will
reflect the likelihood of the block being finalized.

For example:
* 80% of the cluster voted for the block

* The schedule for the next 5 leaders looks like:

    * V,V,V, V,V,V,V, N,N,N,N, V,V,V,V, V,V,V,V

Where V indicates that the leader voted on the block and N indicates
that they haven't.  N is in some partition and N did not observe
the block so N didn't vote. N is not slashed for proposing an
alternative fork, N is allowed to propose forks. Only the validators
that voted for the block can be slashed for proposing fork that
doesn't contain it.


By the time the `N` leader is reached, its likely this block would
be voted on 7 times, and the lockout for this block would prevent
the `N` leader from proposing an alternative fork which doesn't
contain this block.

## Calculating the Probability of a block succeeding

This calculation assumes that the validators are on the same
partition, that the partition is static and that validators will
vote with the same response rate.

To calculate the probability of the block being finalized we would
need to count all the possible ways that future scheduled leaders
will vote and produce blocks.  An approximation of this calculation
is the probability of voting streaks that accumulate enough lockout
to skip over all the non-voting leaders.  A voting streak is as
series of votes that result in the lockout doubling for a block
prior to the streak.

In the above example if the probability of any block being dropped
is 2%, then probability of a streak of size 3 in the next 7 blocks
is 98.07%, and the probability of a streak of size 4 is 97.7%, which
should roughly equal the probability of the fork still being live
after the non-voting leader produced an alternative fork.  Looking
at the leader schedule ahead of the block, the probability of the
block surviving is at least equal to the probability of continuous
run of streaks that keep doubling the lockout.

Each streak needs to produce a lockout that is 2x longer than the
lockout from the previous streak, and each streak has twice as many
blocks in which that possibility can arise. Given that 20% of the
stake weighted leaders are in the wrong partition, a rough estimate
of the likelihood of each streak is as follows:

* streak of size 4 in 7 blocks 97.7%
* streak of size 5 in 12 blocks 98.04%
* streak of size 6 in 25 blocks 98.6%
* streak of size 7 in 51 blocks 99.03%
* streak of size 8 in 108 blocks 99.63%
* streak of size 9 in 204 blocks 99.90%
* streak of size 10 in 409 blocks 99.999%

The cumulative probability is roughly 93.8%.  A better approximation
of this calculation would take into account how the non-voting
leaders are distributed in the schedule.

At any point if all 100% have voted for a descendant, then there
is no way a block could be proposed that doesn't include the parent.
At this point, finality is equal to finality that is achieved when
the cluster reaches the root slot for the block.

## Agreement on Continuing a Fork

A leader is committed to producing a block that is chained to the
last vote the leader cast.  If a leader fails to do so, the leader
will be slashed.

## Slashing for Alternative Forks

* Each block contains a leader signature of the blockhash the block
height and previous block height.

* A proof is composed of the chain of blockhashes starting with the
malicious leader, and a vote for a block the malicious leader not
included in the chain.

## Impact on Liveness

Current implementation leaders already commit to generating a block
that is chained to the previous vote.  The only possible scenario
where this is false is if the leader is restarted and loses data.

* Leaders should store votes in persistent storage before transmitting
them to the cluster.

* Leaders whose persistent storage is corrupted after restart should
vote before submitting a block.

* Leaders that vote for an incorrect bankhash due to a state
corruption are still required to chain their next block to the
previous vote.

## Slashing Size

For this commitment to have the same weight as vote lockouts,
slashing must be equal in severity as slashing for violating a root
fork lockout.

## Notes
* https://www.askamathematician.com/2010/07/q-whats-the-chance-of-getting-a-run-of-k-successes-in-n-bernoulli-trials-why-use-approximations-when-the-exact-answer-is-known/
