# Terminology

## Teminology Currently in Use

The following list contains words commonly used throughout the Solana
architecture.

#### account

A persistent file addressed by [public key](#public-key) and with
[lamports](#lamport) tracking its lifetime.

#### block

A contiguous set of [entries](#entry) on the ledger covered by a
[vote](#ledger-vote).  The length of a block is network hyper parameter,
specified in [ticks](#tick).  Also called [voting period](#voting-period).

#### bootstrap leader

The first [fullnode](#fullnode) to take the [leader](#leader) role.

#### client

A [node](#node) that utilizes the [cluster](#cluster).

#### cluster

A set of [fullnodes](#fullnode) maintaining a single [ledger](#ledger).

#### control plane

A gossip network connecting all [nodes](#node) of a [cluster](#cluster).

#### data plane

A multicast network used to efficiently validate [entries](#entry) and gain
consensus.

#### entry

An entry on the [ledger](#ledger) either a [tick](#tick) or a [transactions
entry](#transactions-entry).

#### escrow account

An account owned by an [escrow service](#escrow-service).

#### escrow service

An interactive [service](#service) that manages [escrow
accounts](#escrow-account) by interpreting client-submited
[instructions](#instruction).

#### confirmation

The wallclock duration between a [leader](#leader) creating a [tick
entry](#tick) and recognizing a supermajority of [ledger votes](#ledger-vote)
with a ledger interpretation that matches the leader's.

#### fork

A [ledger](#ledger) derived from common entries but then diverged.

#### fullnode

A full participant in the [cluster](#cluster) either a [leader](#leader) or
[validator](#validator) node.

#### fullnode state

The result of interpreting the ledger at a given [tick height](#tick-height).
It includes at least the set of all [accounts](#account) holding nonzero
[native tokens](#native-tokens).

#### genesis block

The first [block](#block) of the [ledger](#ledger).

#### hash

A digital fingerprint of a sequence of bytes.

#### instruction

[client](#client)-submitted data that drives a [service](#service).

#### keypair

A [public key](#public-key) and coesponding [secret key](#secret-key).

#### lamport

A fractional [native token](#native-token) with the value of approximately
0.0000000000582 [sol](#sol) (2^-34).

#### loader

A [service](#service) with the ability to interpret the binary encoding of
other services.

#### leader

The role of a [fullnode](#fullnode) when it is appending [entries](#entry) to
the [ledger](#ledger).

#### leader schedule

A sequence of [fullnode](#fullnode) [public keys](#public-key). The cluster
uses the leader schedule to determine which fullnode is the [leader](#leader)
at any moment in time.

#### ledger

A list of [entries](#entry) containing [transactions](#transaction) signed by
[clients](#client).

#### ledger vote

A [hash](#hash) of the [fullnode's state](#fullnode-state) at a given [tick
height](#tick-height). It comprises a validator's affirmation that a
[block](#block) it has received has been verified, as well as a promise not to
vote for a conflicting [block](#block) (i.e. [fork](#fork)) for a specific
amount of time, the [lockout](#lockout) period.

#### lockout

The duration of time for which a [fullnode](#fullnode) is unable to
[vote](#ledger-vote) on another [fork](#fork).

#### native token

The [token](#token) used to track work done by [nodes](#node) in a cluster.

#### node

A computer particpating in a [cluster](#cluster).

#### node count

The number of [fullnodes](#fullnode) participating in a [cluster](#cluster).

#### public key

The public key of a [keypair](#keypair).

#### replicator

A type of [client](#client) that stores copies of segments of the
[ledger](#ledger).

#### secret key

The private key of a [keypair](#keypair).

#### service

The code that interprets client-submited [instructions](#instruction).

#### service ID

The public key of the [account](#account) containing a [service](#service).

#### slot

The time (i.e. number of [blocks](#block)) for which a [leader](#leader)
ingests transactions and produces [entries](#entry).

#### sol

The [native token](#native-token) tracked by a [cluster](#cluster) recognized
by the company Solana.

#### stake

Tokens forfeit to the [cluster](#cluster) if malicious [fullnode](#fullnode)
behavior can be proven.

#### tick

A ledger [entry](#entry) that estimates wallclock duration.

#### tick height

The Nth [tick](#tick) in the [ledger](#ledger).

#### token

A scarce, fungible member of a set of tokens.

#### tps

[Transactions](#transaction) per second.

#### transaction

One or more [instructions](#instruction) signed by the [client](#client) and
executed atomically.

#### transactions entry

A set of [transactions](#transaction) that may be executed in parallel.

#### validator

The role of a [fullnode](#fullnode) when it is validating the
[leader's](#leader) latest [entries](#entry).

#### vote

See [ledger vote](#ledger-vote)

#### voting period

See [block](#block)

## Terminology Reserved for Future Use

The following keywords do not have any functionality but are reserved by Solana
for potential future use.

#### blob

A fraction of a [block](#block); the smallest unit sent between
[fullnodes](#fullnode).

#### curio

A scarce, non-fungible member of a set of curios.

#### epoch

The time, i.e. number of [slots](#slot), for which a [leader
schedule](#leader-schedule) is valid.

#### light client

A type of [client](#client) that can verify it's pointing to a valid
[cluster](#cluster).

#### mips

Millions of [instructions](#instruction) per second.

#### runtime

The component of a [fullnode](#fullnode) responsible for executing
[instructions](#instruction).

#### thin client

A type of [client](#client) that trusts it is communicating with a valid
[cluster](#cluster).
