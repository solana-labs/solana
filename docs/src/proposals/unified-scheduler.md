# Unified Scheduler

## Architechtural problem

- there's two execution scheduling algorithms, and both are subotimmal for current tx load. 
independent algorithms are exposing the so-called replayability risk

also, increasing banking threads doesn't linearly scale to the number of cpu cores.
 
## Current Transaction load pattern

## Redefined scheduler's design goal

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
