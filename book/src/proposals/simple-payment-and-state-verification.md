# Simple Payment and State Verification

It is often useful to allow low resourced clients to participate in a Solana cluster. Be this participation economic or contract execution, verification that a client's activity has been accepted by the network is typically expensive. This proposal lays out a mechanism for such clients to confirm that their actions have been committed to the ledger state with minimal resource expenditure and third-party trust.

## A Naive Approach

Validators store the signatures of recently confirmed transactions for a short period of time to ensure that they are not processed more than once. Validators provide a JSON RPC endpoint, which clients can use to query the cluster if a transaction has been recently processed. Validators also provide a PubSub notification, whereby a client registers to be notified when a given signature is observed by the validator. While these two mechanisms allow a client to verify a payment, they are not a proof and rely on completely trusting a fullnode.

We will describe a way to minimize this trust using Merkle Proofs to anchor the fullnode's response in the ledger, allowing the client to confirm on their own that a sufficient number of their preferred validators have confirmed a transaction. Requiring multiple validator attestations further reduces trust in the fullnode, as it increases both the technical and economic difficulty of compromising several other network participants.

## Light Clients

A 'light client' is a cluster participant that does not itself run a fullnode. This light client would provide a level of security greater than trusting a remote fullnode, without requiring the light client to spend a lot of resources verifying the ledger.

Rather than providing transaction signatures directly to a light client, the fullnode instead generates a Merkle Proof from the transaction of interest to the root of a Merkle Tree of all transactions in the including block. This Merkle Root is stored in a ledger entry which is voted on by validators, providing it consensus legitimacy. The additional level of security for a light client depends on an initial canonical set of validators the light client considers to be the stakeholders of the cluster. As that set is changed, the client can update its internal set of known validators with [receipts](simple-payment-and-state-verification.md#receipts). This may become challenging with a large number of delegated stakes.

Fullnodes themselves may want to use light client APIs for performance reasons. For example, during the initial launch of a fullnode, the fullnode may use a cluster provided checkpoint of the state and verify it with a receipt.

## Receipts

A receipt is a minimal proof that; a transaction has been included in a block, that the block has been voted on by the client's preferred set of validators and that the votes have reached the desired confirmation depth.

The receipts for both state and payments start with a Merkle Path from the value into a Bank-Merkle that has been voted on and included in the ledger. A chain of PoH Entries containing subsequent validator votes, deriving from the Bank-Merkle, is the confirmation proof.

Clients can examine this ledger data and compute the finality using Solana's fork selection rules.

### Payment Merkle Path

A payment receipt is a data structure that contains a Merkle Path from a transaction to the required set of validator votes.

An Entry-Merkle is a Merkle Root including all transactions in the entry, sorted by signature.

![Block Merkle Diagram](../.gitbook/assets/spv-block-merkle.svg)

A Block-Merkle is a Merkle root of all the Entry-Merkles sequenced in the block. Transaction status is necessary for the receipt because the state receipt is constructed for the block. Two transactions over the same state can appear in the block, and therefore, there is no way to infer from just the state whether a transaction that is committed to the ledger has succeeded or failed in modifying the intended state. It may not be necessary to encode the full status code, but a single status bit to indicate the transaction's success.

### State Merkle Path

A state receipt provides a confirmation that a specific state is committed at the end of the block. Inter-block state transitions do not generate a receipt.

For example:

* A sends 5 Lamports to B
* B spends 5 Lamports
* C sends 5 Lamports to A

At the end of the block, A and B are in the exact same starting state, and any state receipt would point to the same value for A or B.

The Bank-Merkle is computed from the Merkle Tree of the new state changes, along with the Previous Bank-Merkle, and the Block-Merkle.

![Bank Merkle Diagram](../.gitbook/assets/spv-bank-merkle.svg)

A state receipt contains only the state changes occurring in the block. A direct Merkle Path to the current Bank-Merkle guarantees the state value at that bank hash, but it cannot be used to generate a “current” receipt to the latest state if the state modification occurred in some previous block. There is no guarantee that the path provided by the validator is the latest one available out of all the previous Bank-Merkles.

Clients that want to query the chain for a receipt of the "latest" state would need to create a transaction that would update the Merkle Path for that account, such as a credit of 0 Lamports.

### Validator Votes

Leaders should coalesce the validator votes by stake weight into a single entry. This will reduce the number of entries necessary to create a receipt.

### Chain of Entries

A receipt has a PoH link from the payment or state Merkle Path root to a list of consecutive validation votes.

It contains the following:

* State -&gt; Bank-Merkle

  or

* Transaction -&gt; Entry-Merkle -&gt; Block-Merkle -&gt; Bank-Merkle

And a vector of PoH entries:

* Validator vote entries
* Ticks
* Light entries

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

The light entries are reconstructed from Entries and simply show the entry Merkle Root that was mixed in to the PoH hash, instead of the full transaction set.

Clients do not need the starting vote state. The [fork selection](https://github.com/solana-labs/solana/tree/aacead62c0eb052068172eba6b53fc85874d6d54/book/src/book/src/fork-selection.md) algorithm is defined such that only votes that appear after the transaction provide finality for the transaction, and finality is independent of the starting state.

### Verification

A light client that is aware of the supermajority set validators can verify a receipt by following the Merkle Path to the PoH chain. The Bank-Merkle is the Merkle Root and will appear in votes included in an Entry. The light client can simulate [fork selection](https://github.com/solana-labs/solana/tree/aacead62c0eb052068172eba6b53fc85874d6d54/book/src/book/src/fork-selection.md) for the consecutive votes and verify that the receipt is confirmed at the desired lockout threshold.

### Synthetic State

Synthetic state should be computed into the Bank-Merkle along with the bank generated state.

For example:

* Epoch validator accounts and their stakes and weights.
* Computed fee rates

These values should have an entry in the Bank-Merkle. They should live under known accounts, and therefore have an exact address in the Merkle Path.

