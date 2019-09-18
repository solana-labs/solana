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
the [genesis block](#genesis-block) has height zero.

#### block id

The [entry id](#entry-id) of the last entry in a [block](#block).

#### bootstrap leader

The first [fullnode](#fullnode) to take the [leader](#leader) role.

#### CBC block

Smallest encrypted chunk of ledger, an encrypted ledger segment would be made of
many CBC blocks. `ledger_segment_size / cbc_block_size` to be exact.

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

#### cooldown period

Some number of epochs after stake has been deactivated while it progressively
becomes available for withdrawal. During this period, the stake is considered to
be "deactivating". More info about:
[warmup and cooldown](stake-delegation-and-rewards.md#stake-warmup-cooldown-withdrawal)

#### credit

See [vote credit](#vote-credit).

#### data plane

A multicast network used to efficiently validate [entries](#entry) and gain
consensus.

#### drone

An off-chain service that acts as a custodian for a user's private key. It
typically serves to validate and sign transactions.

#### fake storage proof

A proof which has the same format as a storage proof, but the sha state is
actually from hashing a known ledger value which the storage client can reveal
and is also easily verifiable by the network on-chain.

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

#### finality

When nodes representing 2/3rd of the stake have a common [root](#root).

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

A [public key](#public-key) and corresponding [private key](#private-key).

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

#### ledger segment

Portion of the ledger which is downloaded by the replicator where storage proof
data is derived.

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

A computer participating in a [cluster](#cluster).

#### node count

The number of [fullnodes](#fullnode) participating in a [cluster](#cluster).

#### PoH

See [Proof of History](#proof-of-history).

#### point

A weighted [credit](#credit) in a rewards regime.  In the validator [rewards regime](staking-rewards.md), the number of points owed to a stake during redemption is the product of the [vote credits](#vote-credit) earned and the number of lamports staked.

#### private key

The private key of a [keypair](#keypair).

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

#### replicator

Storage mining client, stores some part of the ledger enumerated in blocks and
submits storage proofs to the chain. Not a full-node.

#### root

A [block](#block) or [slot](#slot) that has reached maximum [lockout](#lockout)
on a validator.  The root is the highest block that is an ancestor of all active
forks on a validator.  All ancestor blocks of a root are also transitively a
root.  Blocks that are not an ancestor and not a descendant of the root are
excluded from consideration for consensus and can be discarded.


#### runtime

The component of a [fullnode](#fullnode) responsible for [program](#program)
execution.

#### slot

The period of time for which a [leader](#leader) ingests transactions and
produces a [block](#block).

#### smart contract

A set of constraints that once satisfied, signal to a program that some
predefined account updates are permitted.

#### sol

The [native token](#native-token) tracked by a [cluster](#cluster) recognized
by the company Solana.

#### stake

Tokens forfeit to the [cluster](#cluster) if malicious [fullnode](#fullnode)
behavior can be proven.

#### storage proof

A set of sha hash state which is constructed by sampling the encrypted version
of the stored ledger segment at certain offsets.

#### storage proof challenge

A transaction from a replicator that verifiably proves that a validator
confirmed a fake proof.

#### storage proof claim

A transaction from a validator which is after the timeout period given from the
storage proof confirmation and which no successful challenges have been
observed which rewards the parties of the storage proofs and confirmations.

#### storage proof confirmation

A transaction by a validator which indicates the set of real and fake proofs
submitted by a storage miner. The transaction would contain a list of proof
hash values and a bit which says if this hash is valid or fake.

#### storage validation capacity

The number of keys and samples that a validator can verify each storage epoch.

#### sysvar

A synthetic [account](#account) provided by the runtime to allow programs to
access network state such as current tick height, rewards [points](#point) values, etc.

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

#### vote credit

A reward tally for validators.  A vote credit is awarded to a validator in its
vote account when the validator reaches a [root](#root).

#### warmup period

Some number of epochs after stake has been delegated while it progressively
becomes effective. During this period, the stake is considered to be
"activating". More info about:
[warmup and cooldown](stake-delegation-and-rewards.md#stake-warmup-cooldown-withdrawal)
