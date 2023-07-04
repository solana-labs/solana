---
title: Simple Payment and State Verification
---

It is often useful to allow low resourced clients to participate in a Solana
cluster. Be this participation economic or contract execution, verification
that a client's activity has been accepted by the network is typically
expensive. This proposal lays out a mechanism for such clients to confirm that
their actions have been committed to the ledger state with minimal resource
expenditure and third-party trust.

## A Naive Approach

Validators store the signatures of recently confirmed transactions for a short
period of time to ensure that they are not processed more than once. Validators
provide a JSON RPC endpoint, which clients can use to query the cluster if a
transaction has been recently processed. Validators also provide a PubSub
notification, whereby a client registers to be notified when a given signature
is observed by the validator. While these two mechanisms allow a client to
verify a payment, they are not a proof and rely on completely trusting a
validator.

We will describe a way to minimize this trust using Merkle Proofs to anchor the
validator's response in the ledger, allowing the client to confirm on their own
that a sufficient number of their preferred validators have confirmed a
transaction. Requiring multiple validator attestations further reduces trust in
the validator, as it increases both the technical and economic difficulty of
compromising several other network participants.

## Light Clients

A 'light client' is a cluster participant that does not itself run a validator.
This light client would provide a level of security greater than trusting a
remote validator, without requiring the light client to spend a lot of resources
verifying the ledger.

Rather than providing transaction signatures directly to a light client, the
validator instead generates a Merkle Proof from the transaction of interest to
the root of a Merkle Tree of all transactions in the including block. This
Merkle Root is stored in a ledger entry which is voted on by validators,
providing it consensus legitimacy. The additional level of security for a light
client depends on an initial canonical set of validators the light client
considers to be the stakeholders of the cluster. As that set is changed, the
client can update its internal set of known validators with
[receipts](simple-payment-and-state-verification.md#receipts). This may become
challenging with a large number of delegated stakes.

Validators themselves may want to use light client APIs for performance reasons.
For example, during the initial launch of a validator, the validator may use a
cluster provided checkpoint of the state and verify it with a receipt.

## Receipts

A receipt is a minimal proof that; a transaction has been included in a block,
that the block has been voted on by the client's preferred set of validators
and that the votes have reached the desired confirmation depth.

### Transaction Inclusion Proof

A transaction inclusion proof is a data structure that contains a Merkle Path
from a transaction, through an Entry-Merkle to a Block-Merkle, which is included
in a Bank-Hash with the required set of validator votes. A chain of PoH Entries
containing subsequent validator votes, deriving from the Bank-Hash, is the proof
of confirmation.

#### Transaction Merkle

An Entry-Merkle is a Merkle Root including all transactions in a given entry,
sorted by signature. Each transaction in an entry is already merkled here:
https://github.com/solana-labs/solana/blob/b6bfed64cb159ee67bb6bdbaefc7f833bbed3563/ledger/src/entry.rs#L205.
This means we can show a transaction `T` was included in an entry `E`.

A Block-Merkle is the Merkle Root of all the Entry-Merkles sequenced in the
block.

![Block Merkle Diagram](/img/spv-block-merkle.svg)

Together the two merkle proofs show a transaction `T` was included in a block
with bank hash `B`.

An Accounts-Hash is the hash of the concatenation of the state hashes of
each account modified during the current slot.

Transaction status is necessary for the receipt because the state receipt is
constructed for the block. Two transactions over the same state can appear in
the block, and therefore, there is no way to infer from just the state whether
a transaction that is committed to the ledger has succeeded or failed in
modifying the intended state. It may not be necessary to encode the full status
code, but a single status bit to indicate the transaction's success.

Currently, the Block-Merkle is not implemented, so to verify `E` was an entry
in the block with bank hash `B`, we would need to provide all the entry hashes
in the block. Ideally this Block-Merkle would be implmented, as the alternative
is very inefficient.

#### Block Headers

In order to verify transaction inclusion proofs, light clients need to be able
to infer the topology of the forks in the network

More specifically, the light client will need to track incoming block headers
such that given two bank hashes for blocks `A` and `B`, they can determine
whether `A` is an ancestor of `B` (Below section on
`Optimistic Confirmation Proof` explains why!). Contents of header are the
fields necessary to compute the bank hash.

A Bank-Hash is the hash of the concatenation of the Block-Merkle and
Accounts-Hash described in the `Transaction Merkle` section above.

![Bank Hash Diagram](/img/spv-bank-hash.svg)

In the code:

https://github.com/solana-labs/solana/blob/b6bfed64cb159ee67bb6bdbaefc7f833bbed3563/runtime/src/bank.rs#L3468-L3473

```
        let mut hash = hashv(&[
            // bank hash of the parent block
            self.parent_hash.as_ref(),
            // hash of all the modifed accounts
            accounts_delta_hash.hash.as_ref(),
            // Number of signatures processed in this block
            &signature_count_buf,
            // Last PoH hash in this block
            self.latest_blockhash().as_ref(),
        ]);
```

A good place to implement this logic along existing streaming logic in the
validator's replay logic: https://github.com/solana-labs/solana/blob/b6bfed64cb159ee67bb6bdbaefc7f833bbed3563/core/src/replay_stage.rs#L1092-L1096

#### Optimistic Confirmation Proof

Currently optimistic confirmation is detected via a listener that monitors
gossip and the replay pipeline for votes:
https://github.com/solana-labs/solana/blob/b6bfed64cb159ee67bb6bdbaefc7f833bbed3563/core/src/cluster_info_vote_listener.rs#L604-L614.

Each vote is a signed transaction that includes the bank hash of the block the
validator voted for, i.e. the `B` from the `Transaction Merkle` section above.
Once a certain threshold `T` of the network has voted on a block, the block is
considered optimistially confirmed. The votes made by this group of `T`
validators is needed to show the block with bank hash `B` was optimistically
confirmed.

However other than some metadata, the signed votes themselves are not
currently stored anywhere, so they can't be retrieved on demand. These votes
probably need to be persisted in Rocksdb database, indexed by a key
`(Slot, Hash, Pubkey)` which represents the slot of the vote, bank hash of the
vote, and vote account pubkey responsible for the vote.

Together, the transaction merkle and optimistic confirmation proofs can be
provided over RPC to subscribers by extending the existing signature
subscrption logic. Clients who subscribe to the "Confirmed" confirmation
level are already notified when optimistic confirmation is detected, a flag
can be provided to signal the two proofs above should also be returned.

It is important to note that optimistcally confirming `B` also implies that all
ancestor blocks of `B` are also optimistically confirmed, and also that not
all blocks will be optimistically confirmed.

```

B -> B'

```

So in the example above if a block `B'` is optimisically confirmed, then so is
`B`. Thus if a transaction was in block `B`, the transaction merkle in the
proof will be for block `B`, but the votes presented in the proof will be for
block `B'`. This is why the headers in the `Block headers` section above are
important, the client will need to verify that `B` is indeed an ancestor of
`B'`.

#### Proof of Stake Distribution

Once presented with the transaction merkle and optimistic confirmation proofs
above, a client can verify a transaction `T` was optimistially confirmed in a
block with bank hash `B`. The last missing piece is how to verify that the
votes in the optimistic proofs above actually constitute the valid `T`
percentage of the stake necessay to uphold the safety guarantees of
"optimistic confirmation".

One way to approach this might be for every epoch, when the stake set changes,
to write all the stakes to a system account, and then have validators subscribe
to that system account. Full nodes can then provide a merkle proving that the
system account state was updated in some block `B`, and then show that the
block `B` was optimistically confirmed/rooted.

### Account State Verification

An account's state (balance or other data) can be verified by submitting a
transaction with a **_TBD_** Instruction to the cluster. The client can then
use a [Transaction Inclusion Proof](#transaction-inclusion-proof) to verify
whether the cluster agrees that the acount has reached the expected state.

### Validator Votes

Leaders should coalesce the validator votes by stake weight into a single entry.
This will reduce the number of entries necessary to create a receipt.

### Chain of Entries

A receipt has a PoH link from the payment or state Merkle Path root to a list
of consecutive validation votes.

It contains the following:

- Transaction -&gt; Entry-Merkle -&gt; Block-Merkle -&gt; Bank-Hash

And a vector of PoH entries:

- Validator vote entries
- Ticks
- Light entries

```text
/// This Entry definition skips over the transactions and only contains the
/// hash of the transactions used to modify PoH.
LightEntry {
    /// The number of hashes since the previous Entry ID.
    pub num_hashes: u64,
    /// The SHA-256 hash `num_hashes` after the previous Entry ID.
    hash: Hash,
    /// The Merkle Root of the transactions encoded into the Entry.
    entry_hash: Hash,
}
```

The light entries are reconstructed from Entries and simply show the entry
Merkle Root that was mixed in to the PoH hash, instead of the full transaction
set.

Clients do not need the starting vote state. The
[fork selection](../implemented-proposals/tower-bft.md) algorithm is defined
such that only votes that appear after the transaction provide finality for the
transaction, and finality is independent of the starting state.

### Verification

A light client that is aware of the supermajority set validators can verify a
receipt by following the Merkle Path to the PoH chain. The Block-Merkle is the
Merkle Root and will appear in votes included in an Entry. The light client can
simulate [fork selection](../implemented-proposals/tower-bft.md) for the
consecutive votes and verify that the receipt is confirmed at the desired
lockout threshold.

### Synthetic State

Synthetic state should be computed into the Bank-Hash along with the bank
generated state.

For example:

- Epoch validator accounts and their stakes and weights.
- Computed fee rates

These values should have an entry in the Bank-Hash. They should live under known
accounts, and therefore have an index into the hash concatenation.
