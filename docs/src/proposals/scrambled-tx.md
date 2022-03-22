---
title: Scrambled Transaction to combat against MEV
---

### Program Statement

The existence of MEV isn't ideal and costly.

MEV motivates transaction reordering, censoring and copying. Also, it causes
cluster destabilization because of incentivised forking.

The root problem is that there is no rule in the pursuit of MEV frontier, except
economic rationality. While this anarchism is welcomed, stemmed from the
blockchain philosophy, it's also desirable for such an activity to economically
impractical in the first place.

Any temporal market imbalance should be corrected strictly on the grounds of a
fair game, basically the golden first-come-first-served rule (with optional
bidding for priority).

### Summary of Proposed Solution

This proposal tries to combat against MEV, at the cost of (slight) global
induced latency of cluster as a whole.

At highest level, a full mitigation of MEV can be attained _if leaders (and
searchers) can't know the contents of other's transactions until finishing to
packing and ordering them into a new block_. In addition to this, the
transaction details must also be deterministically revealed after the ordering
of transactions is committed.

So, naive 2-step transaction submission (encrypted tx, then its key) won't work,
let alone increased latency.

Instead, this proposal uses a special transaction construction _only to delay
the revelation of its content for the short amount of time tied to confirmation
by the cluster_. Specifically, transactions are encrypted by a private key
derived from the recursive sha256, and its seed and cipher text are submitted to
the cluster at the same time.

This construction is called *scrambling* across this proposal.

Conversely, the forced computation to reveal the transaction details by solving
the recursive sha256 is called *descrambling*.

### Summary of consensus changes

In addition to the obvious change of submission of scrambled transactions by
clients, this proposal introduces a new pipeline stage (called the descrambling
stage) and splits the voting into 2 phases (pre-vote and vote).

Firstly, the leader feeds incoming transactions down the turbine immediately
with minimum sanitization (This assumes leaders are now so-called bank-less and
scheduling is delayed and on-chain via mempool).

The validators pre-vote on the completed block, once they recover all data.
Thus, the opt-conf commitment of data availability of the block can be rather
established rather quickly, without replay timing variation by all other
validators (This average duration will be the basis of difficulty adjustment of
scrambling).

Also, rpc can notify optimistic execution signal to clients at this point. This
signifies subsequent pipeline processing is fully deterministic and not reverted
unless opt-conf is violated (some portion of stake is slash-able).

At the same time, the validators also initiate the descrambling stage for the
block in a random, collaborative and distributed manner. Once they solve their
chosen chunk of scrambled transactions, they send revealed encryption keys to
the leader to be propagated via turbine likewise the block shreds. Also
sig-verify is done at the end of descrambling.

After all transactions are descrambled from the cooperative efforts, the block
can be finally scheduled and executed deterministically.

Lastly, validators votes on the reproduced bank hash after replaying the
processing likewise. Then, rpc can notify optimistic confirmation signal to
clients.

### Downsides

- Increased latency
- Some computational cost on clients
- Reliance of local wall clock time
- Pretty invasive system change

### Scrambling details

As said above, scrambling refers to a specially-arranged encryption scheme where
anyone can decrypt (= reveal) the transaction details after a non-parallelizable
computation which takes certain amount of time.

On top of it, scrambling also has a nice property where the revelation can
easily be verified by anyone with some kind of message authentication tag. This
means rpc node doesn't need GPU.

All of fields in a transaction are scrambled with randomized padding, including
fee payer to avoid any kind of fingerprinting while allowing garbage-based
spamming with amplification risk.

Also, scrambled transaction expires very quickly based on wall clock time,
usually less than the targeted duration of difficulty to protest against
processing stalls by malicious leaders for MEV.

So, given the raw transaction binary:

```
[transaction details]
```

, scrambling encloses it like this:

```
[seed to recursive sha256]
[recursion count]
[encrypted authentication tag]
[encrypted expiration unix time]
[encrypted transaction details]
```

So, this assumes quic to bypass the transaction udp packet size limit.

The number of sha256 recursion is chosen so that anyone can't reveal the
transaction details, until the cluster optimistically commits the ordering of
transactions in a block with very high certainty. So, this assumes the recursive
sha256 is exhibits rather uniform throughput across various hardware (cpu/gpu)
on the market.

For instance, difficulty can be adjusted 6 9s of availability of MEV-immunity
status based on standard deviation of past conf time. or hard-coded initially.

Scrambling is optional. While execution of not-scrambled transaction is stalled
still, infallible and/or MEV-free simple transactions can opt not to be
scrambled because scrambling might not be desirable for all of situations (HD
wallets and program deploy transactions).

### Scrambling preparation by clients

Clients need to prepare valid scrambling challenge secrets. By definition, they
have no choice but to compute the recursive sha256, starting from
locally-available secure random as seed. However, the challenge secrets are
designed to be pre-computed before the actual transaction construction/signing.
So, they can prepare those challenge secrets in background and idling manner for
later use.

### 2 step voting and voting lock-out

pre-vote and vote are expected to be coalesced to avoid doubling the consensus
communication overhead. This means a single voting transaction will include
instructions to pre-vote on block N and to vote on block N-1. This shouldn't
badly affect latency because block time is still 400ms frequency. Also, the
pre-vote instruction must contain timestamp.

pre-vote and vote on block N must occur in order. And voting rules including
lockout are generalized as if any vote comprises of the 2 steps. So fork-choice
and related subsystem isn't changed significantly.

### Latency analysis

Scrambling will inevitably increases latency. However this should be tolerable
in trade for the MEV immunity.

Descrambling difficulty can be targeted to be 2x of current opt-conf duration as
a very crude approach. The 2x is needed as a safe buffer (TODO: needs proper
statistical analysis).

However this is quite inefficient and too naive.

Firstly, the current opt-conf duration is encompassing all the timings,
including block propagation, sig/entry verify, transaction replay and vote
accumulation. The new pre-vote opt-conf duration should be a small portion of
it, namely the block propagation and vote accumulation.

We can also exclude the vote accumulation duration because the block propagation
duration is the only duration which needs to remain scrambled. We can securely
derive the stake-weighted duration of block propagation from timestamps in past
pre-votes (could be hard-coded initially) on epoch basis.

Lastly, block propagation should finish within a slot or two, otherwise cluster
isn't healthy in the first place.

Thus, approximately 4 slots (~1.6s) will be scrambling target difficulty. This
is major component of induced latency. In addition to this, scrambled keys must
also propagated via turbine while being pipelined. This will also add ~2 slots
latency, totaling ~6 slots (~2.4s).

As before, latencies from sig/entry verify, replay, and vote accumulation still
exist. However scrambling by itself won't cause pipeline stalls. Thus there
should be no more latency sources, other than described in the previous
paragraph.

### Attack scenarios

#### Spamming by clients

Unfortunately, forging invalid scrambled transaction is very easy. So, the
cluster is susceptible to spamming, possibly with intensive wasteful
computation. However, considering 1500 validators on mainnet-beta, the cluster
can be extremely redundant, in terms of GPU resource. So, network bandwidth will
be saturated firstly and combating the generic DDoS is out of scope for this
proposal.

#### Stalling a block production via turbine by leaders

This attack can be mounted in the hopes of reordering transactions after
descrambling. However, this is prevented by the very short wall-clock based
expiration attached to scrambled transactions. Also, pre-vote is equipped with
wall clock to ignore the whole block if this particular opt-conf duration for
the block is longer than the target duration of scrambling.

#### Stalling revealed key propagation via turbine by leaders

Naturally, leaders want their blocks to be included in the major fork. However,
malice leader can still annoy the cluster by stalling the descrambling phase
in this way. To mitigate this, we allow subsequent leaders can also propagate
the revealed keys shreds.

#### Stalling the banking stage after full revelation of scrambled transactions

Likewise above, the mitigation here is that the next leader can replay the
previous leader's block if the previous leader has silenced.

### Optimal behaviors for each role

#### Clients

They don't want to be targeted by MEV for any trading transactions.
Thereby they scramble the transaction and never share the derived private key to
descramble. Also, they can't withhold revelation of their scrambled transaction
as its outcome turned out unfavorable. Also, they want to protect their
transactions from cluster stalls. So, they make transactions quickly expire.
Thus, their strategy for most likely favorable trade is submission of their
transaction as fast as possible as determined.

Lastly, transactions are now included into a block very greedily. And
observation by the cluster can be signalled very reliably. So, clients (esp.
bots) are less incentivised to spamming for the race of inclusion.

#### Leaders

They want to maximize profit from running the validator in any possible ways.
So, they try to stall the block production for MEV with the help of searchers.
However, stalling block production is guaranteed to result in skipped block. All
in all, this behavior incurs opportunity cost. Thus, their optimal behavior is
include transactions indiscriminately as many as possible for maximum block
rewards. This naturally results in the FIFO ordering of transactions.

#### Validators

They want cluster fork less frequently so generally cooperates.
Unless significant nodes (like more than 1/3) are colluding, they pre-vote
blocks honestly with accurate wall clock in the fear of reduced rewards because
of being outstanding outlier otherwise.

Also they're incentivised for publishing scrambled transactions as first solver
by small rewards. Lying the results is eligible for slashing to ensure bad
scrambled transaction is detected and backed by stakes.

### Further extensions

This proposal lays down the foundation for further protocol extension.

Firstly, this whole new descrambling process can be regarded as a deterministic
and async commit-and-reveal. This means a non-interactive, unbiasble,
deterministic and free random number can be provided at the consensus layer as
by-product, with a few tweaks of this proposal.

That random can easily be fed into transaction runtime for one thing. Further,
this random number is also used to combat against verifiers's dilemma (or
tragedy of commons) as well, because of the very existence at the consensus
layer, opening fierce yet cooperative validator competition.

All in all, the induced latency should be worth as the trade-off to realize
these technical breakthroughs.

### Prior work

None? This scrambling is leveraging the fact Solana's very short block time and
abundance of online GPU resources.

### Remaining concerns

- Unsolved MEV opportunity on the on-chain mempool. So clients should be wise to
  value their transaction with adequate additional priority fee.

- Reveal of intension from failed scrambled transaction. For that concern,
  strong (= finger-printing resistant) scrambling is needed. also, this can tip
  about more-or-less immediate general market sentiment change after some news
  release, even if these transactions are individually protected from
  reordering?

- sha256-ing throughput difference between cpu and gpu?

- mev searchers will compete on the on-chain mempool with on-chain transaction
  submission mechanism as new battle frontier.
