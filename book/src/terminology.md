# Terminology

The following terms are used throughout this book.

#### account

A persistent file addressed by [public key](#public-key) and with
[lamports](#lamport) tracking its lifetime.

#### app

A front-end application that interacts with a Solana cluster.

#### blob

A fraction of a [block](#block); the smallest unit sent between
[fullnodes](#fullnode).

#### block

A contiguous set of [entries](#entry) on the ledger covered by a
[vote](#ledger-vote). A [leader](#leader) produces at most one block per
[slot](#slot).

#### block height

The number of [blocks](#block) beneath the current block. The first block after
the [genesis block](#genesis block) has height zero.

#### block id

The [entry id](#entry-id) of the last entry in a [block](#block).

#### bootstrap leader

The first [fullnode](#fullnode) to take the [leader](#leader) role.

#### client

A [node](#node) that utilizes the [cluster](#cluster).

#### cluster

A set of [fullnodes](#fullnode) maintaining a single [ledger](#ledger).

#### confirmation

The wallclock duration between a [leader](#leader) creating a [tick
entry](#tick) and recognizing a supermajority of [ledger votes](#ledger-vote)
with a ledger interpretation that matches the leader's.

#### control plane

A gossip network connecting all [nodes](#node) of a [cluster](#cluster).

#### data plane

A multicast network used to efficiently validate [entries](#entry) and gain
consensus.

#### drone

An off-chain service that acts as a custodian for a user's private key. It
typically serves to validate and sign transactions.

#### entry

An entry on the [ledger](#ledger) either a [tick](#tick) or a [transactions
entry](#transactions-entry).

#### entry id

A globally unique identifier that is also a proof that the [entry](#entry) was
generated after a duration of time, all [transactions](#transaction) included
in the entry, and all previous entries on the [ledger](#ledger). See [Proof of
History](#proof-of-history).

#### epoch

The time, i.e. number of [slots](#slot), for which a [leader
schedule](#leader-schedule) is valid.

#### fork

A [ledger](#ledger) derived from common entries but then diverged.

#### fullnode

A full participant in the [cluster](#cluster) either a [leader](#leader) or
[validator](#validator) node.

#### fullnode state

The result of interpreting all programs on the ledger at a given [tick
height](#tick-height). It includes at least the set of all [accounts](#account)
holding nonzero [native tokens](#native-tokens).

#### genesis block

The configuration file that prepares the [ledger](#ledger) for the first [block](#block).

#### hash

A digital fingerprint of a sequence of bytes.

#### instruction

The smallest unit of a [program](#program) that a [client](#client) can include
in a [transaction](#instruction).

#### keypair

A [public key](#public-key) and corresponding [secret key](#secret-key).

#### lamport

A fractional [native token](#native-token) with the value of approximately
0.0000000000582 [sol](#sol) (2^-34).

#### loader

A [program](#program) with the ability to interpret the binary encoding of
other on-chain programs.

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

#### light client

A type of [client](#client) that can verify it's pointing to a valid
[cluster](#cluster). It performs more ledger verification than a [thin
client](#thin-client) and less than a [fullnode](#fullnode).

#### lockout

The duration of time for which a [fullnode](#fullnode) is unable to
[vote](#ledger-vote) on another [fork](#fork).

#### native token

The [token](#token) used to track work done by [nodes](#node) in a cluster.

#### node

A computer particpating in a [cluster](#cluster).

#### node count

The number of [fullnodes](#fullnode) participating in a [cluster](#cluster).

#### PoH

See [Proof of History](#proof-of-history).

#### program

The code that interprets [instructions](#instruction).

#### program id

The public key of the [account](#account) containing a [program](#program).

#### Proof of History

A stack of proofs, each which proves that some data existed before the proof
was created and that a precise duration of time passed before the previous
proof. Like a [VDF](#verifiable-delay-function), a Proof of History can be
verified in less time than it took to produce.

#### public key

The public key of a [keypair](#keypair).

#### runtime

The component of a [fullnode](#fullnode) responsible for [program](#program)
execution.

#### secret key

The private key of a [keypair](#keypair).

#### slot

The period of time for which a [leader](#leader) ingests transactions and
produces a [block](#block).

#### sol

The [native token](#native-token) tracked by a [cluster](#cluster) recognized
by the company Solana.

#### stake

Tokens forfeit to the [cluster](#cluster) if malicious [fullnode](#fullnode)
behavior can be proven.

#### thin client

A type of [client](#client) that trusts it is communicating with a valid
[cluster](#cluster).

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

#### VDF

See [verifiable delay function](#verifiable-delay-function).

#### verifiable delay function

A function that takes a fixed amount of time to execute that produces a proof
that it ran, which can then be verified in less time than it took to produce.

#### vote

See [ledger vote](#ledger-vote).
