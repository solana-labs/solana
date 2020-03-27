# Terminology

The following terms are used throughout the documentation.

## account

A persistent file addressed by [public key](terminology.md#public-key) and with [lamports](terminology.md#lamport) tracking its lifetime.

## app

A front-end application that interacts with a Solana cluster.

## bank state

The result of interpreting all programs on the ledger at a given [tick height](terminology.md#tick-height). It includes at least the set of all [accounts](terminology.md#account) holding nonzero [native tokens](terminology.md#native-tokens).

## block

A contiguous set of [entries](terminology.md#entry) on the ledger covered by a [vote](terminology.md#ledger-vote). A [leader](terminology.md#leader) produces at most one block per [slot](terminology.md#slot).

## blockhash

A preimage resistant [hash](terminology.md#hash) of the [ledger](terminology.md#ledger) at a given [block height](terminology.md#block-height). Taken from the last [entry id](terminology.md#entry-id) in the slot

## block height

The number of [blocks](terminology.md#block) beneath the current block. The first block after the [genesis block](terminology.md#genesis-block) has height one.

## bootstrap validator

The first [validator](terminology.md#validator) to produce a [block](terminology.md#block).

## CBC block

Smallest encrypted chunk of ledger, an encrypted ledger segment would be made of many CBC blocks. `ledger_segment_size / cbc_block_size` to be exact.

## client

A [node](terminology.md#node) that utilizes the [cluster](terminology.md#cluster).

## cluster

A set of [validators](terminology.md#validator) maintaining a single [ledger](terminology.md#ledger).

## confirmation time

The wallclock duration between a [leader](terminology.md#leader) creating a [tick entry](terminology.md#tick) and creating a [confirmed block](terminology.md#confirmed-block).

## confirmed block

A [block](terminology.md#block) that has received a [supermajority](terminology.md#supermajority) of [ledger votes](terminology.md#ledger-vote) with a ledger interpretation that matches the leader's.

## control plane

A gossip network connecting all [nodes](terminology.md#node) of a [cluster](terminology.md#cluster).

## cooldown period

Some number of epochs after stake has been deactivated while it progressively becomes available for withdrawal. During this period, the stake is considered to be "deactivating". More info about: [warmup and cooldown](cluster/stake-delegation-and-rewards.md#stake-warmup-cooldown-withdrawal)

## credit

See [vote credit](terminology.md#vote-credit).

## data plane

A multicast network used to efficiently validate [entries](terminology.md#entry) and gain consensus.

## drone

An off-chain service that acts as a custodian for a user's private key. It typically serves to validate and sign transactions.

## entry

An entry on the [ledger](terminology.md#ledger) either a [tick](terminology.md#tick) or a [transactions entry](terminology.md#transactions-entry).

## entry id

A preimage resistant [hash](terminology.md#hash) over the final contents of an entry, which acts as the [entry's](terminology.md#entry) globally unique identifier. The hash serves as evidence of:

 * The entry being generated after a duration of time
 * The specified [transactions](terminology.md#transaction) are those included in the entry
 * The entry's position with respect to other entries in [ledger](terminology.md#ledger)

See [Proof of History](terminology.md#proof-of-history).

## epoch

The time, i.e. number of [slots](terminology.md#slot), for which a [leader schedule](terminology.md#leader-schedule) is valid.

## fake storage proof

A proof which has the same format as a storage proof, but the sha state is actually from hashing a known ledger value which the storage client can reveal and is also easily verifiable by the network on-chain.

## fee account

The fee account in the transaction is the account pays for the cost of including the transaction in the ledger. This is the first account in the transaction. This account must be declared as Read-Write (writable) in the transaction since paying for the transaction reduces the account balance.

## finality

When nodes representing 2/3rd of the stake have a common [root](terminology.md#root).

## fork

A [ledger](terminology.md#ledger) derived from common entries but then diverged.

## genesis block

The first [block](terminology.md#block) in the chain.

## genesis config

The configuration file that prepares the [ledger](terminology.md#ledger) for the [genesis block](terminology.md#genesis-block).

## hash

A digital fingerprint of a sequence of bytes.

## inflation

An increase in token supply over time used to fund rewards for validation and replication and to fund continued development of Solana.

## instruction

The smallest unit of a [program](terminology.md#program) that a [client](terminology.md#client) can include in a [transaction](terminology.md#instruction).

## keypair

A [public key](terminology.md#public-key) and corresponding [private key](terminology.md#private-key).

## lamport

A fractional [native token](terminology.md#native-token) with the value of 0.000000001 [sol](terminology.md#sol).

## leader

The role of a [validator](terminology.md#validator) when it is appending [entries](terminology.md#entry) to the [ledger](terminology.md#ledger).

## leader schedule

A sequence of [validator](terminology.md#validator) [public keys](terminology.md#public-key). The cluster uses the leader schedule to determine which validator is the [leader](terminology.md#leader) at any moment in time.

## ledger

A list of [entries](terminology.md#entry) containing [transactions](terminology.md#transaction) signed by [clients](terminology.md#client).

## ledger segment

Portion of the ledger which is downloaded by the archiver where storage proof data is derived.

## ledger vote

A [hash](terminology.md#hash) of the [validator's state](terminology.md#bank-state) at a given [tick height](terminology.md#tick-height). It comprises a validator's affirmation that a [block](terminology.md#block) it has received has been verified, as well as a promise not to vote for a conflicting [block](terminology.md#block) \(i.e. [fork](terminology.md#fork)\) for a specific amount of time, the [lockout](terminology.md#lockout) period.

## light client

A type of [client](terminology.md#client) that can verify it's pointing to a valid [cluster](terminology.md#cluster). It performs more ledger verification than a [thin client](terminology.md#thin-client) and less than a [validator](terminology.md#validator).

## loader

A [program](terminology.md#program) with the ability to interpret the binary encoding of other on-chain programs.

## lockout

The duration of time for which a [validator](terminology.md#validator) is unable to [vote](terminology.md#ledger-vote) on another [fork](terminology.md#fork).

## native token

The [token](terminology.md#token) used to track work done by [nodes](terminology.md#node) in a cluster.

## node

A computer participating in a [cluster](terminology.md#cluster).

## node count

The number of [validators](terminology.md#validator) participating in a [cluster](terminology.md#cluster).

## PoH

See [Proof of History](terminology.md#proof-of-history).

## point

A weighted [credit](terminology.md#credit) in a rewards regime. In the validator [rewards regime](proposals/staking-rewards.md), the number of points owed to a stake during redemption is the product of the [vote credits](terminology.md#vote-credit) earned and the number of lamports staked.

## private key

The private key of a [keypair](terminology.md#keypair).

## program

The code that interprets [instructions](terminology.md#instruction).

## program id

The public key of the [account](terminology.md#account) containing a [program](terminology.md#program).

## Proof of History

A stack of proofs, each which proves that some data existed before the proof was created and that a precise duration of time passed before the previous proof. Like a [VDF](terminology.md#verifiable-delay-function), a Proof of History can be verified in less time than it took to produce.

## public key

The public key of a [keypair](terminology.md#keypair).

## archiver

Storage mining client, stores some part of the ledger enumerated in blocks and submits storage proofs to the chain. Not a validator.

## root

A [block](terminology.md#block) or [slot](terminology.md#slot) that has reached maximum [lockout](terminology.md#lockout) on a validator. The root is the highest block that is an ancestor of all active forks on a validator. All ancestor blocks of a root are also transitively a root. Blocks that are not an ancestor and not a descendant of the root are excluded from consideration for consensus and can be discarded.

## runtime

The component of a [validator](terminology.md#validator) responsible for [program](terminology.md#program) execution.

## shred

A fraction of a [block](terminology.md#block); the smallest unit sent between [validators](terminology.md#validator).

## slot

The period of time for which a [leader](terminology.md#leader) ingests transactions and produces a [block](terminology.md#block).

## smart contract

A set of constraints that once satisfied, signal to a program that some predefined account updates are permitted.

## sol

The [native token](terminology.md#native-token) tracked by a [cluster](terminology.md#cluster) recognized by the company Solana.

## stake

Tokens forfeit to the [cluster](terminology.md#cluster) if malicious [validator](terminology.md#validator) behavior can be proven.

## storage proof

A set of sha hash state which is constructed by sampling the encrypted version of the stored ledger segment at certain offsets.

## storage proof challenge

A transaction from an archiver that verifiably proves that a validator confirmed a fake proof.

## storage proof claim

A transaction from a validator which is after the timeout period given from the storage proof confirmation and which no successful challenges have been observed which rewards the parties of the storage proofs and confirmations.

## storage proof confirmation

A transaction by a validator which indicates the set of real and fake proofs submitted by a storage miner. The transaction would contain a list of proof hash values and a bit which says if this hash is valid or fake.

## storage validation capacity

The number of keys and samples that a validator can verify each storage epoch.

## supermajority

2/3 of a [cluster](terminology.md#cluster).

## sysvar

A synthetic [account](terminology.md#account) provided by the runtime to allow programs to access network state such as current tick height, rewards [points](terminology.md#point) values, etc.

## thin client

A type of [client](terminology.md#client) that trusts it is communicating with a valid [cluster](terminology.md#cluster).

## tick

A ledger [entry](terminology.md#entry) that estimates wallclock duration.

## tick height

The Nth [tick](terminology.md#tick) in the [ledger](terminology.md#ledger).

## token

A scarce, fungible member of a set of tokens.

## tps

[Transactions](terminology.md#transaction) per second.

## transaction

One or more [instructions](terminology.md#instruction) signed by the [client](terminology.md#client) and executed atomically.

## transaction confirmations

The number of [confirmed blocks](terminology.md#confirmed-block) since the transaction was accepted onto the [ledger](terminology.md#ledger). A transaction is finalized when its block becomes a [root](terminology.md#root).

## transactions entry

A set of [transactions](terminology.md#transaction) that may be executed in parallel.

## validator

A full participant in the [cluster](terminology.md#cluster) responsible for validating the [ledger](terminology.md#ledger) and producing new [blocks](terminology.md#block).

## VDF

See [verifiable delay function](terminology.md#verifiable-delay-function).

## verifiable delay function

A function that takes a fixed amount of time to execute that produces a proof that it ran, which can then be verified in less time than it took to produce.

## vote

See [ledger vote](terminology.md#ledger-vote).

## vote credit

A reward tally for validators. A vote credit is awarded to a validator in its vote account when the validator reaches a [root](terminology.md#root).

## wallet

A collection of [keypairs](terminology.md#keypair).

## warmup period

Some number of epochs after stake has been delegated while it progressively becomes effective. During this period, the stake is considered to be "activating". More info about: [warmup and cooldown](cluster/stake-delegation-and-rewards.md#stake-warmup-cooldown-withdrawal)
