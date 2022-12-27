---
title: Timely Vote Credits
---

## Timely Vote Credits

This design describes a modification to the method that is used to calculate
vote credits earned by validator votes.

Vote credits are the accounting method used to determine what percentage of
inflation rewards a validator earns on behalf of its stakers.  Currently, when
a slot that a validator has previously voted on is "rooted", it earns 1 vote
credit.  A "rooted" slot is one which has received full committment by the
validator (i.e. has been finalized).

One problem with this simple accounting method is that it awards one credit
regardless of how "old" the slot that was voted on at the time that it was
voted on.  This means that a validator can delay its voting for many slots in
order to survey forks and wait to make votes that are less likely to be
expired, and without incurring any penalty for doing so.  This is not just a
theoretical concern: there are known and documented instances of validators
using this technique to significantly delay their voting while earning more
credits as a result.


### Proposed Change

The proposal is to award a variable number of vote credits per voted on slot,
with more credits being given for votes that have "less latency" than votes
that have "more latency".

In this context, "latency" is the number of slots in between the slot that is
being voted on and the slot in which the vote has landed.  Because a slot
cannot be voted on until after it has been completed, the minimum possible
latency is 1, which would occur when a validator voted as quickly as possible,
transmitting its vote on that slot in time for it to be included in the very
next slot.

Credits awarded would become a function of this latency, with lower latencies
awarding more credits.  This will discourage intentional "lagging", because
delaying a vote for any slots decreases the number of credits that vote will
earn, because it will necessarily land in a later slot if it is delayed, and
then earn a lower number of credits than it would have earned had it been
transmitted immediately and landed in an earlier slot.

### Grace Period

If landing a vote with 1 slot latency awarded more credit than landing that
same vote in 2 slots latency, then validators who could land votes
consistently wihthin 1 slot would have a credits earning advantage over those
who could not.  Part of the latency when transmitting votes is unavoidable as
it's a function of geographical distance between the sender and receiver of
the vote.  The Solana network is spread around the world but it is not evenly
distributed over the whole planet; there are some locations which are, on
average, more distant from the network than others are.

It would likely be harmful to the network to encourage tight geographical
concentration - if, for example, the only way to achieve 1 slot latency was to
be within a specific country - then a very strict credit rewards schedule
would encourage all validators to move to the same country in order to
maximize their credit earnings.

For this reason, the credits reward schedule should have a built-in "grace
period" that gives all validators a "reasonable" amount of time to land their
votes.  This will reduce the credits earning disadvantage that comes from
being more distant from the network.  A balance needs to be struck between the
strictest rewards schedule, which most strongly discourages intentional
lagging, and more lenient rewards schedules, which improves credit earnings
for distant validators who are not artificially lagging.

Historical voting data has been analyzed over many epochs and the data
suggests that the smallest grace period that allows for very minimal impact on
well behaved distant validators is 3 slots, which means that all slots voted
on within 3 slots will award maximum vote credits to the voting validator.
This gives validators nearly 2 seconds to land their votes without penalty.
The maximum latency between two points on Earth is about 100 ms, so allowing a
full 1,500 ms to 2,000 ms latency without penalty should not have adverse
impact on distant validators.

### Maximum Vote Credits

Another factor to consider is what the maximum vote credits to award for a
vote should be.  Assuming linear reduction in vote credits awarded (where 1
slot of additional lag reduces earned vote credits by 1), the maximum vote
credits value determines how much "penalty" there is for each additional slot
of latency.  For example, a value of 10 would mean that after the grace period
slots, every additional slot of latency would result in a 10% reduction in
vote credits earned as each subsequent slot earns 1 credit less out of a
maximum possible 10 credits.

Again, historical voting data was analyzed over many epochs and the conclusion
drawn was that a maximum credits of 10 is the largest value that can be used
and still have a noticeable effect on known laggers.  Values higher than that
result in such a small penalty for each slot of lagging that intentional
lagging is still too profitable.  Lower values are even more punishing to
intentional lagging; but an attempt has been made to conservatively choose the
highest value that produces noticeable results.

The selection of these values is partially documented here:

https://www.shinobi-systems.com/timely_voting_proposal

The above document is somewhat out of date with more recent analysis, which
occurred in this github issue:

https://github.com/solana-labs/solana/issues/19002

To summarize the findings of these documents: analysis over many epochs showed
that almost all validators from all regions have an average vote latency of 1
slot or less.  The validators with higher average latency are either known
laggers, or are not representative of their region since many other validators
in the same region achieve low latency voting.  With a maximum vote credit of
10, there is almost no change in vote credits earned relative to the highest vote
earner by the majority of validators, aside from a general uplift of about 0.4%.
Additionally, data centers were analyzed to ensure that there aren't regions of
the world that would be adversely affected, and none were found.


### Method of Implementation

When a Vote or VoteStateUpdate instruction is received by a validator, it will
use the Clock sysvar to identify the slot in which that instruction has
landed.  For any newly voted on slot within that Vote or VoteStateUpdate
transaction, the validator will record the vote latency of that slot as
(voted_in_slot - voted_on_slot).

These vote latencies will be stored a new vector of u8 latency values appended
to the end of the VoteState.  VoteState currently has ~200 bytes of free space
at the end that is unused, so this new vector of u8 values should easily fit
within this available space.  Because VoteState is an ABI frozen structure,
utilizing the mechanisms for updating frozen ABI will be required, which will
complicate the change.  Furthermore, because VoteState is embedded in the
Tower data structure and it is frozen ABI as well, updates to the frozen ABI
mechanisms for Tower will be needed also.  These are almost entirely
mechanical changes though, that involve ensuring that older versions of these
data structures can be updated to the new version as they are read in, and the
new version written out when the data structure is next persisted.

The credits to award for a rooted slot will be calculated using the latency
value stored in latency vector for the slot, and a formula that awards
latencies of 1 - 3 slots ten credits, with a 1 credit reduction for each vote
latency after 3.  Rooted slots will always be awarded a minimum credit of 1
(never 0) so that very old votes, possibly necessary in times of network
stress, are not discouraged.

To summarize the above: latency is recorded in a new Vector at the end of
VoteState when a vote first lands, but the credits for that slot are not
awarded until the slot becomes rooted, at which point the latency that was
recorded is used to compute the credits to award for that newly rooted slot.

When a Vote instruction is processed, the changes are fairly easy to implement
as Vote can only add new slots to the end of Lockouts and pop existing slots
off of the back (which become rooted), so the logic merely has to compute
rewards for the new roots, and new latencies for the newly added slots, both
of which can be processed in the fairly simple existing logic for Vote
processing.

When a VoteStateUpdate instruction is processed:

1. For each slot that was in the previous VoteState but are not in the new
VoteState because they have been rooted in the transition from the old
VoteState to the new VoteState, credits to award are calculated based on the
latency that was recorded for them and still available in the old VoteState.

2. For each slot that was in both the previous VoteState and the new
VoteState, the latency that was previously recorded for that slot is copied
from the old VoteState to the new VoteState.

3. For each slot that is in the new VoteState but wasn't in the old VoteState,
the latency value is calculated for this new slot according to what slot the
vote is for and what slot is in the Clock (i.e. the slot this VoteStateUpdate
tx landed in) and this latency is stored in VoteState for that slot.

The code to handle this is more complex, because VoteStateUpdate may include
removal of slots that expired as performed by the voting validator, in
addition to slots that have been rooted and new slots added.  However, the
assumptions that are needed to handle VoteStateUpdate with timely vote credits
are already guaranteed by existing VoteStateUpdate correctness checking code:

The existing VoteStateUpdate processing code already ensures that (1) only
roots slots that could actually have been rooted in the transition from the
old VoteState to the new VoteState, so there is no danger of over-counting
credits (i.e. imagine that a 'cheating' validator "pretended" that slots were
rooted by dropping them off of the back of the new VoteState before they have
actually achieved 32 confirmations; the existing logic prevents this).

The existing VoteStateUpdate processing code already ensures that (2) new
slots included in the new VoteState are only slots after slots that have
already been voted on in the old VoteState (i.e. can't inject new slots in the
middle of slots already voted on).
