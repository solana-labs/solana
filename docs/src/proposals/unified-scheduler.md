# Unified Scheduler

## Terminologies

## Architechtural problem

- there's two execution scheduling algorithms, and both are subotimmal for current tx load. 
independent algorithms are exposing the so-called replayability risk

also, increasing replaying/banking threads doesn't linearly scale to the number of cpu cores.
 
## Present (and projected) transaction patterns

Simply put, the current transaction pattern is extremely diversified in terms
of various aspects of system resource usage. This is a stark difference since
existing implementations were designed/implemented originally, warranting to
rework on the area.

Firstly, it's safe assumption that transaction execution's wall-clock duration
will differ by 100x: from spl-token transfer's ~50us, to heavily-corss-margined
liquidation's ~5ms. These are due to the nature of inherent complexity
differences of these respective atomic state transitions, which are reasonable
for both of mentioned use cases. Thus, even after all upcoming optimizations
(like `direct_mapping`) are in place, it's expected for these variance to
persist for foreseeable future.

Regarding discussion of scheduler's design, it's also important to note that
transaction's address access pattern varies greatly as well. Simple
transactions access a handful, while others will do 50s (soon, up to 256) of
them, thanks to the recent introduction of Address lookup table mechanism.
Also, when seen from the viewpoint of the on-chain state, a very few of its
addresses can be highly contended _chronically_ (i.e order books) or _acutely_
(i.e. IDO/NFT drop), while vast of others are seldom accessed.

To make the situation more nuanced, consensus messages (= vote transactions)
are currently included into blocks (i.e. on-chain) likewise the normal
transactions. These collectively comprise a noticeable presence among the
overall system load and are characterized as being free of lock contentions,
fast to execute, quite large in quantities, and inherently high-priority.

All in all, any upcoming changes to the scheduler must accommodate to the
divergence of these peculiar load pattern. At the same time, it shouldn't be
over-optimized for the current pattern, introducing heuristics and/or fairness
skew.  That's because any blockchain network's scheduling imbalance can be
exploited by malicious users. It should strictly strive for being
generic/adaptive, not like other problem settings (i.e. trusted environments).
That means, synthesized benchmark results should be taken with a grain of salt
because they tend to be overly uniform, not reflecting the realistic usage.

Lastly, it also can assumed that there will always be more pending non-vote
transactions than system's capacity. From theoretical perspective, that would
be because of [Parkinson's law](https://en.wikipedia.org/wiki/Parkinson%27s_law#Generalization).
From empirical perspective, this should have been true for very long time.

## Redefined scheduler's problem space

In general terms, leaders are expected to pack as many as transactions to
maximize profits. On the flip side, users are expected to pay as few as fees to
minimize costs. There should be no exception for rational participants in this
transaction-fee market.

That said, it can be characterized that this _market_ is arising from the
scarcity of _time slice_ in the case of Solana, unlike the so-called
_blockspace_ in others.

That property can be derived from the simple and unforgiving fact that Solana's
block propagation must be streamed _in real time_ due to competition among
leaders. Any stalemate would be forced to be less-populated blocks, because
that block won't be finished to be replayed otherwise, due to the wasted time.
Then, such blocks will be regarded as less favorable to vote by others (might
not the case at the moment due to current fork choice, but ideally adjustments
should be made for this to be held true for the maximum utility of the cluster
itself, economically speaking). At the end of story, that behavior would
adversely affect the consequential likeliness of block confirmation by other
validators.  Block proposal timings are quite severe in Solana and should be so
to realize its promised very low-latency.

Then, it can now be said leaders are gaming to pack transactions _not to create
idling **time** of blocktime (`slot` in solana)_, rather than _not to create
empty **space** of blockspace_.

That limits transaction reordering capability severely due to this very tight
time constraints. In other words, searching for more lucrative transaction
arrangement in the realam of NP-complete solution is just risking the binary
opportunity cost. That's because this activity would be regarded against the
best interest of whole cluster. Then, supermajority of others are eager to
punish these observable selfish behavior by means of vote abstain.

So, the game can be simplified to blindly try to saturate the blocktime with
highest-paying (= premium/time) available transactions for any given moment.
Concretely, that saturation is defined to fill the 400ms of slot time as long
as others can replay likewise. (note: That means this proposal is against
bankless leader proposal)

Then, at this later part of this section, we finally introduce the fact the
Solana's program execution is multi-threaded by nature. That extension is
delayed intentionally not to complicate preceding explanation and it's rather
straightforward to extend. Firstly, the current assumption of unbounded number
of execution threads isn't viable for supermajority's replayability. So, some
bounded core count _N_ must be hard-coded, picked from the present common node
setup for both staked and rpc nodes.
 
Then, block saturation is defined as `N*CU` where N is such core count and CU
is equivalent of 400ms for single-threaded on-chain program execution.
Currently, compute unit are used to meter block limits indirectly. `N*CU`
becomes the unit of measurement of _saturation_ of blocks, which can be called
_blockspacetime_ (_space_ here refers to the discrete dimension of `N`)

This shift of scheduling doctrine should also lead to a compromise of ongoing
scheduling dilemma due to Solana's multi-threaded nature:
_throughput-optimized_ (i.e. fee collection maximization, favored by
validators) vs _latency-optimized_ (i.e. priority adherence maximization,
favored by clients). The former is meaningfully impractical due to the
aforementioned real-time constraints. So, throughput-optimization can be
reduced to packing non-overlapping transactions as much as possible in the
order of fees, equating to the later.  This middle ground compromised strategy
can be defined as _utilization-optimized_ scheduling.

All these observation should warrant the justification of alternative
scheduler.

## Proposed Solution

## why unify?: because simply using the same algo makes most sense.

## Currnet implementation problem

- why batching isn't good?
- what about block max compute unit?

## Proposed implementation

### high-level scheduling algorithm

eventual successful locking.

pessimic locking is chosen for now (might switch to optimistic)


### design philophy

- no timer/no bucketing (thus, no jitter and no odd juggy incentive)
- no huetrics (try to be generic/agnostic/adaptive as much as possible)
- no estimator/prediction (so bots can't trick leaders)
- single threaded
- determinicity
- strict fairness, only considering priority_fee/fcfs
- approx. O(n) where n is gross total of addresses in transactions.
- strict adherance to local fee market
- censorship resistent
- 100k scheduling/s
- highly contended address with 1m pending txes doesn't affect overall performance at all.
- offload rpc-related to post-processing

### high level code organization

SchedulerPool => Scheduler => SchedulingMachine

off loader;

#### some tricks or tactical optimizatons for replay stage

- entry index of txes as the priroity 
- high-priority lane for contended queue

#### ditto for banking

this is planned or only lightly experimented

- just route voting txes as highest priority.
- skiplist/provitional locking/stale cu

#### why single threaded?

all the good benefits except the single and obvious pitfall: can become bottleneck.

no pathologic poor performance due to heavy lock contention for small subset of active addresses

#### overhead of tx-level scheduling


## release steps

- replay stage first as transaction batch with single tx
- replace banking
- more tight integrations

## economic/security analysis

incentive compatibility for max-profit-seeking leaders and cheapest-fee-seeking users.
censorship attack

# risks

scheduler thread could be the bottleneck

## further work
- rework compute unit, consider runtime account state transition verification/N addresses (i.e. scheduling cost)
- what is going with the bankless?: meh
- scrambled tx
- loopback transaction submission from on-chain programs. no cranks/liquidation/arb bots?
- repurpose this even for forwading. (this should be applicable in terms of network schduling as well)
