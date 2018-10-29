Initial Proof of Stake (PoS) (i.e. using in-protocol asset, SOL, to provide secure consensus) design ideas outlined here. Solana will implement a proof of stake reward/security scheme for node validators on the network. The purpose is threefold:

- Align validator incentives with that of the greater network through skin-in-the-game deposits at risk
- Avoid 'nothing at stake' branch voting issues by implementing slashing rules aimed at promoting branch convergence
- Provide an avenue for validator rewards provided as a function of validator participation in the network.

While many of the details of the specific implementation are currently under consideration and are expected to come into focus through specific modeling studies and parameter exploration on the Solana testnet, we outline here our current thinking on the main components of the PoS system. Much of this thinking is based on the current status of Casper FFG, with optimizations and specific attributes to be modified as is allowed by Solana's Proof of History (PoH) blockchain data structure.

### General Overview

Solana's validation network design is based on a rotating, stake-weighted randomly selected leader broadcasting network transactions in a PoH data structure to validating nodes. These nodes, upon receiving the leader's broadcast, have the opportunity to vote on the current state and PoH height by signing a transaction into the PoH stream.

To become a Solana validator, a network node must deposit/lock-up some amount of SOL in a contract. This SOL will not be accessible for a specific time period. The precise duration of the staking lockup period has not been determined. However we can consider three phases of this time for which specific parameters will be necessary:

- *Warm-up period*: which SOL is deposited and inaccessible to the node, however PoH transaction validation has not begun. Most likely on the order of days to weeks
- *Validation period*: a minimum duration for which the deposited SOL will be inaccessible, at risk of slashing (see slashing rules below) and earning network rewards for the validator participation. Likely duration of months to a year.
- *Cool-down period*: a duration of time following the submission of a 'withdrawal' transaction. During this period validation responsibilities have been removed and the funds continue to be inaccessible. Accumulated rewards should be delivered at the end of this period, along with the return of the initial deposit.

Solana's trustless sense of time and ordering provided by its PoH data structure, along with its [avalanche](https://www.youtube.com/watch?v=qt_gDRXHrHQ&t=1s) data broadcast and transmission design, should provide sub-second finality times that scale with the log of the number of nodes in the network. This means we shouldn't have to restrict the number of validating nodes with a prohibitive 'minimum deposits' and expect nodes to be able to become validators with nominal amounts of SOL staked. This should also render validation pools, a proposed solution for economic censorship imposed by minimum staking amounts currently described in Casper, unnecessary and remove the concern for needing to put slashable stake at risk while relying on others to play by the rules.

### Stake-weighted Rewards

Rewards are expected to be paid out to active validators as a function of validator activity and as a proportion of the percentage of SOL they have at stake out of the entirety of the staking pool.

We expect to define a baseline annual validator payout/inflation rate based on the total SOL deposited. E.g. 10% annual interest on SOL deposited with X total SOL deposited as slashable on network. This is the same design as currently proposed in Casper FFG which has additionally specifies how inflation rates adjust as a function of total ETH deposited. Specifically, Casper validator returns are proportional to the inverse square root of the total deposits and initial annual rates are estimated as:

| Deposit Size | Annual Validator Interest |
|--------------|---------------------------|
| 2.5M ETH     | 10.12%                    |
| 10M ETH      | 5.00%                     |
| 20M ETH      | 3.52%                     |
| 40M ETH      | 2.48%                     |

This has the nice property of potentially incentivizing participation around a target deposit size. Incentivisation of specific participation rates more directly (rather than deposit size) may something also worth exploring.

The specifics of the Solana validator reward scheme are to be worked out in parallel with a design for transaction fee assignment as well as our storage mining reward scheme.

### Slashing rules

Unlike Proof of Work (PoW) where off-chain capital expenses are already deployed at the time of block construction/voting, PoS systems require capital-at-risk to prevent a logical/optimal strategy of multiple chain voting. We intend to implement slashing rules which, if broken, result some amount of the offending validator's deposited stake to be removed from circulation. Given the ordering properties of the PoH data structure, we believe we can simplify our slashing rules to the level of a voting lockout time assigned per vote.  

I.e. Each vote has an associated lockout time (PoH duration) that represents a duration by any additional vote from that validator must be in a PoH that contains the original vote, or a portion of that validator's stake is slashable. This duration time is a function of the initial vote PoH count and all additional vote PoH counts.  It will likely take the form:

Lockout<sub>i</sub>(PoH<sub>i</sub>, PoH<sub>j</sub>) = PoH<sub>j</sub> + K * exp((PoH<sub>j</sub> - PoH<sub>i</sub>) / K)

Where PoH<sub>i</sub> is the height of the vote that the lockout is to be applied to and PoH<sub>j</sub> is the height of the current vote on the same branch. If the validator submits a vote on a different PoH branch on any PoH<sub>k</sub> where k > j > i and PoH<sub>k</sub> < Lockout(PoH<sub>i</sub>, PoH<sub>j</sub>), then a portion of that validator's stake is at risk of being slashed.

In addition to the functional form lockout described above, early implementation may be a numerical approximation based on a First In, First Out (FIFO) data structure and the following logic:
- FIFO queue holding 32 votes per active validator
- new votes are pushed on top of queue (`push_front`)
- expired votes are popped off top (`pop_front`)
- as votes are pushed into the queue, the lockout of each queued vote doubles
- votes are removed from back of queue if `queue.len() > 32`
- the earliest and latest height that has been removed from the back of the queue should be stored

It is likely that a reward will be offered as a % of the slashed amount to any node that submits proof of this slashing condition being violated to the PoH.

#### Partial Slashing
In the schema described so far, when a validator votes on a given PoH stream, they are committing themselves to that branch for a time determined by the vote lockout. An open question is whether validators will be hesitant to begin voting on an available branch if the penalties are perceived too harsh for an honest mistake or flipped bit.

One way to address this concern would be a partial slashing design that results in a slashable amount as a function of either:

1) the fraction of validators, out of the total validator pool, that were also slashed during the same time period (ala Casper),
2) the amount of time since the vote was cast (e.g. a linearly increasing % of total deposited as slashable amount over time), or both.  

This is an area currently under exploration


### Penalties
As previously discussed, annual validator reward rates are to be specified as a function of total amount staked. The network rewards validators who are online and actively participating in the validation process throughout the entirety of their *validation period*. For validators that go offline/fail to validate transactions during this period, their annual reward is effectively reduced.

Similarly, we may consider an algorithmic reduction in a validator's active amount staked amount in the case that they are offline. I.e. if a validator is inactive for some amount of time, either due to a partition or otherwise, the amount of their stake that is considered ‘active’ (eligible to earn rewards) may be reduced. This design would be structured to help long-lived partitions to eventually reach finality on their respective chains as the % of non-voting total stake is reduced over time until a super-majority can be achieved by the active validators in each partition. Similarly, upon re-engaging, the ‘active’ amount staked will come back online at some defined rate. Different rates of stake reduction may be considered depending on the size of the partition/active set.
