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
over-optimized for the current pattern, introducing heuristics and/or fairness skew.
That's because any blockchain network's scheduling imbalance can be exploited by
malicious users. It should strictly strive for being generic/adaptive, not
like other problem settings (i.e. trusted environments).

That means, synthesized benchmark results should be taken with a grain of salt
because they tend to be overly uniform, not reflecting the realistic usage.

## Redefined scheduler's problem space

In general terms, leaders are expected to pack (i.e. after scheduling by their likes) as many as transactions to maximize profits. On the flip side, users are expected to pay as few as fees to minimize costs. There should be no exception for rational participants in this market.

That said, it can be characterized that this _market_ is actually of _time slice_ in the case of Solana, unlike the so-called _blockspace_ in others.

That property can be derived from the simple and unforgiving fact that Solana's block propagation must be streamed _in real time_ due to competition among leaders. Any stalemate would be forced to be less-populated because that block won't be finished to be replayed otherwise due to the wasted time. Then, such blocks will be regarded as less favorable to vote by others (might not the case at the moment due to current fork choice, but ideally adjustments should be made for this to be held true for the maximum utility of the cluster itself, economically). As a whole, that illogical behavior would adversely affect the consequential likeliness of block confirmation by other validators. Thereby, block proposal timings are quite severe in Solana.

Then, it can now be said leader are gaming to pack transactions _not to create idling **time** of blocktime (`slot` in solana)_, rather than _not to create empty **space** of blockspace_.

That limits transactions reordering capability severely due to this very tight time constraicts. Waiting for more lucrative transactions are just risking the binary opportunity cost, because supermajority of others are eager to punish these observable selfish behavior by means of vote abstrain. Clearly, that would be regarded against the best interest of whole cluster.

The game is simplified to blindly try to saturate the blocktime with highest-paying transactions for any given moment of time (premium/time paid by users). In concreate terms, that saturation is defined to fill the 400ms as long as others can replay likewise. (note: That means this proposal is against bankless leader proposal)

At this later part of this section, we finally introduce the fact the solana's program execution is multithreaded by nature. That extension is delayed intentionally not to complite preceeding reasoning and it's rather straightforward. Firstly, unboudned execution threads isn't viable for supermajority's replayability. So, some bounded core count N must be hard-coded picked from the present common node setup to both staked and rpc nodes.
 
Then, block saturation is defined as N*CU where N is such core count and CU is equivalent of 400ms for single-threaed on-chain program execution. Currently, compute unit are used to meter these two dimentions ambiguously in somewhat unclear definition. Now, CU's definition is refined and N*CU becomes the unit of measurement of saturation, which can be called like _blockspacetime_.

Fianlly, all these observation should lead to the justification of alternative scheduler.



In other words, some users are willing to pay little more than others to use it. 

This means leaders must propose blocks timely at closer look.

thread utilization-optimized

saturate N cores with full of tx exec cycles.

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
