---
title: "Transaction Confirmation"
---

Problems relating to [transaction confirmation](./../terminology.md#transaction-confirmations) are common with many newer developers while building applications. This article aims to boost the overall understanding of the confirmation mechanism used on the Solana blockchain, including some recommended best practices.

## Brief background on transactions

Let’s first make sure we’re all on the same page and thinking about the same things...

### What is a transaction?

Transactions consist of two components: a [message](./../terminology.md#message) and a [list of signatures](./../terminology.md#signature). The transaction message is where the magic happens and at a high level it consists of three components:

- a **list of instructions** to invoke,
- a **list of accounts** to load, and
- a **“recent blockhash.”**

In this article, we’re going to be focusing a lot on a transaction’s [recent blockhash](./../terminology.md#blockhash) because it plays a big role in transaction confirmation.

### Transaction lifecycle refresher

Below is a high level view of the lifecycle of a transaction. This article will touch on everything except steps 1 and 4.

1. Create a list of instructions along with the list of accounts that instructions need to read and write
2. Fetch a recent blockhash and use it to prepare a transaction message
3. Simulate the transaction to ensure it behaves as expected
4. Prompt user to sign the prepared transaction message with their private key
5. Send the transaction to an RPC node which attempts to forward it to the current block producer
6. Hope that a block producer validates and commits the transaction into their produced block
7. Confirm the transaction has either been included in a block or detect when it has expired

## What is a Blockhash?

A [“blockhash”](./../terminology.md#blockhash) refers to the last Proof of History (PoH) hash for a [“slot”](./../terminology.md#slot) (description below). Since Solana uses PoH as a trusted clock, a transaction’s recent blockhash can be thought of as a **timestamp**.

### Proof of History refresher

Solana’s Proof of History mechanism uses a very long chain of recursive SHA-256 hashes to build a trusted clock. The “history” part of the name comes from the fact that block producers hash transaction id’s into the stream to record which transactions were processed in their block.

[PoH hash calculation](https://github.com/solana-labs/solana/blob/9488a73f5252ad0d7ea830a0b456d9aa4bfbb7c1/entry/src/poh.rs#L82): `next_hash = hash(prev_hash, hash(transaction_ids))`

PoH can be used as a trusted clock because each hash must be produced sequentially. Each produced block contains a blockhash and a list of hash checkpoints called “ticks” so that validators can verify the full chain of hashes in parallel and prove that some amount of time has actually passed. The stream of hashes can be broken up into the following time units:

# Transaction Expiration

By default, all Solana transactions will expire if not committed to a block in a certain amount of time. The **vast majority** of transaction confirmation issues are related to how RPC nodes and validators detect and handle **expired** transactions. A solid understanding of how transaction expiration works should help you diagnose the bulk of your transaction confirmation issues.

## How does transaction expiration work?

Each transaction includes a “recent blockhash” which is used as a PoH clock timestamp and expires when that blockhash is no longer “recent” enough. More concretely, Solana validators look up the corresponding slot number for each transaction’s blockhash that they wish to process in a block. If the validator [can’t find a slot number for the blockhash](https://github.com/solana-labs/solana/blob/9488a73f5252ad0d7ea830a0b456d9aa4bfbb7c1/runtime/src/bank.rs#L3687) or if the looked up slot number is more than 151 slots lower than the slot number of the block being processed, the transaction will be rejected.

Slots are configured to last about [400ms](https://github.com/solana-labs/solana/blob/47b938e617b77eb3fc171f19aae62222503098d7/sdk/program/src/clock.rs#L12) but often fluctuate between 400ms and 600ms, so a given blockhash can only be used by transactions for about 60 to 90 seconds.

Transaction has expired pseudocode: `currentBankSlot > slotForTxRecentBlockhash + 151`

Transaction not expired pseudocode: `currentBankSlot - slotForTxRecentBlockhash < 152`

### Example of transaction expiration

Let’s walk through a quick example:

1. A validator is producing a new block for slot #1000
2. The validator receives a transaction with recent blockhash `1234...` from a user
3. The validator checks the `1234...` blockhash against the list of recent blockhashes leading up to its new block and discovers that it was the blockhash for slot #849
4. Since slot #849 is exactly 151 slots lower than slot #1000, the transaction hasn’t expired yet and can still be processed!
5. But wait, before actually processing the transaction, the validator finished the block for slot #1000 and starts producing the block for slot #1001 (validators get to produce blocks for 4 consecutive slots).
6. The validator checks the same transaction again and finds that it’s now too old and drops it because it’s now 152 slots lower than the current slot :(

## Why do transactions expire?

There’s a very good reason for this actually, it’s to help validators avoid processing the same transaction twice.

A naive brute force approach to prevent double processing could be to check every new transaction against the blockchain’s entire transaction history. But by having transactions expire after a short amount of time, validators only need to check if a new transaction is in a relatively small set of _recently_ processed transactions.

### Other blockchains

Solana’s approach of prevent double processing is quite different from other blockchains. For example, Ethereum tracks a counter (nonce) for each transaction sender and will only process transactions that use the next valid nonce.

Ethereum’s approach is simple for validators to implement, but it can be problematic for users. Many people have encountered situations when their Ethereum transactions got stuck in a _pending_ state for a long time and all the later transactions, which used higher nonce values, were blocked from processing.

### Advantages on Solana

There are a few advantages to Solana’s approach:

1. A single fee payer can submit multiple transactions at the same time that are allowed to be processed in any order. This might happen if you’re using multiple applications at the same time.
2. If a transaction doesn’t get committed to a block and expires, users can try again knowing that their previous transaction won’t ever be processed.

By not using counters, the Solana wallet experience may be easier for users to understand because they can get to success, failure, or expiration states quickly and avoid annoying pending states.

### Disadvantages on Solana

Of course there are some disadvantages too:

1. Validators have to actively track a set of all processed transaction id’s to prevent double processing.
2. If the expiration time period is too short, users might not be able to submit their transaction before it expires.

These disadvantages highlight a tradeoff in how transaction expiration is configured. If the expiration time of a transaction is increased, validators need to use more memory to track more transactions. If expiration time is decreased, users don’t have enough time to submit their transaction.

Currently, Solana clusters require that transactions use blockhashes that are no more than [151 slots](https://github.com/solana-labs/solana/blob/9488a73f5252ad0d7ea830a0b456d9aa4bfbb7c1/sdk/program/src/clock.rs#L65) old.

> This [Github issue](https://github.com/solana-labs/solana/issues/23582) contains some calculations that estimate that mainnet-beta validators need about 150MB of memory to track transactions.
> This could be slimmed down in the future if necessary without decreasing expiration time as I’ve detailed in that issue.

## Transaction confirmation tips

As mentioned before, blockhashes expire after a time period of only 151 slots which can pass as quickly as **one minute** when slots are processed within the target time of 400ms.

One minute is not a lot of time considering that a client needs to fetch a recent blockhash, wait for the user to sign, and finally hope that the broadcasted transaction reaches a leader that is willing to accept it. Let’s go through some tips to help avoid confirmation failures due to transaction expiration!

### Fetch blockhashes with the appropriate commitment level

Given the short expiration time frame, it’s imperative that clients help users create transactions with blockhash that is as recent as possible.

When fetching blockhashes, the current recommended RPC API is called [`getLatestBlockhash`](/api/http#getlatestblockhash). By default, this API uses the `"finalized"` commitment level to return the most recently finalized block’s blockhash. However, you can override this behavior by [setting the `commitment` parameter](/api/http#configuring-state-commitment) to a different commitment level.

**Recommendation**

The `"confirmed"` commitment level should almost always be used for RPC requests because it’s usually only a few slots behind the `"processed"` commitment and has a very low chance of belonging to a dropped [fork](./../cluster/fork-generation.md).

But feel free to consider the other options:

- Choosing `"processed"` will let you fetch the most recent blockhash compared to other commitment levels and therefore gives you the most time to prepare and process a transaction. But due to the prevalence of forking in the Solana protocol, roughly 5% of blocks don’t end up being finalized by the cluster so there’s a real chance that your transaction uses a blockhash that belongs to a dropped fork. Transactions that use blockhashes for abandoned blocks won’t ever be considered recent by any blocks that are in the finalized blockchain.
- Using the default commitment level `"finalized"` will eliminate any risk that the blockhash you choose will belong to a dropped fork. The tradeoff is that there is typically at least a 32 slot difference between the most recent confirmed block and the most recent finalized block. This tradeoff is pretty severe and effectively reduces the expiration of your transactions by about 13 seconds but this could be even more during unstable cluster conditions.

### Use an appropriate preflight commitment level

If your transaction uses a blockhash that was fetched from one RPC node then you send, or simulate, that transaction with a different RPC node, you could run into issues due to one node lagging behind the other.

When RPC nodes receive a `sendTransaction` request, they will attempt to determine the expiration block of your transaction using the most recent finalized block or with the block selected by the `preflightCommitment` parameter. A **VERY** common issue is that a received transaction’s blockhash was produced after the block used to calculate the expiration for that transaction. If an RPC node can’t determine when your transaction expires, it will only forward your transaction **one time** and then will **drop** the transaction.

Similarly, when RPC nodes receive a `simulateTransaction` request, they will simulate your transaction using the most recent finalized block or with the block selected by the `preflightCommitment` parameter. If the block chosen for simulation is older than the block used for your transaction’s blockhash, the simulation will fail with the dreaded “blockhash not found” error.

**Recommendation**

Even if you use `skipPreflight`, **ALWAYS** set the `preflightCommitment` parameter to the same commitment level used to fetch your transaction’s blockhash for both `sendTransaction` and `simulateTransaction` requests.

### Be wary of lagging RPC nodes when sending transactions

When your application uses an RPC pool service or when the RPC endpoint differs between creating a transaction and sending a transaction, you need to be wary of situations where one RPC node is lagging behind the other. For example, if you fetch a transaction blockhash from one RPC node then you send that transaction to a second RPC node for forwarding or simulation, the second RPC node might be lagging behind the first.

**Recommendation**

For `sendTransaction` requests, clients should keep resending a transaction to a RPC node on a frequent interval so that if an RPC node is slightly lagging behind the cluster, it will eventually catch up and detect your transaction’s expiration properly.

For `simulateTransaction` requests, clients should use the [`replaceRecentBlockhash`](/api/http#simulatetransaction) parameter to tell the RPC node to replace the simulated transaction’s blockhash with a blockhash that will always be valid for simulation.

### Avoid reusing stale blockhashes

Even if your application has fetched a very recent blockhash, be sure that you’re not reusing that blockhash in transactions for too long. The ideal scenario is that a recent blockhash is fetched right before a user signs their transaction.

**Recommendation for applications**

Poll for new recent blockhashes on a frequent basis to ensure that whenever a user triggers an action that creates a transaction, your application already has a fresh blockhash that’s ready to go.

**Recommendation for wallets**

Poll for new recent blockhashes on a frequent basis and replace a transaction’s recent blockhash right before they sign the transaction to ensure the blockhash is as fresh as possible.

### Use healthy RPC nodes when fetching blockhashes

By fetching the latest blockhash with the `"confirmed"` commitment level from an RPC node, it’s going to respond with the blockhash for the latest confirmed block that it’s aware of. Solana’s block propagation protocol prioritizes sending blocks to staked nodes so RPC nodes naturally lag about a block behind the rest of the cluster. They also have to do more work to handle application requests and can lag a lot more under heavy user traffic.

Lagging RPC nodes can therefore respond to blockhash requests with blockhashes that were confirmed by the cluster quite awhile ago. By default, a lagging RPC node detects that it is more than 150 slots behind the cluster will stop responding to requests, but just before hitting that threshold they can still return a blockhash that is just about to expire.

**Recommendation**

Monitor the health of your RPC nodes to ensure that they have an up-to-date view of the cluster state with one of the following methods:

1. Fetch your RPC node’s highest processed slot by using the [`getSlot`](/api/http#getslot) RPC API with the `"processed"` commitment level and then call the [`getMaxShredInsertSlot](/api/http#getmaxshredinsertslot) RPC API to get the highest slot that your RPC node has received a “shred” of a block for. If the difference between these responses is very large, the cluster is producing blocks far ahead of what the RPC node has processed.
2. Call the `getLatestBlockhash` RPC API with the `"confirmed"` commitment level on a few different RPC API nodes and use the blockhash from the node that returns the highest slot for its [context slot](/api/http#rpcresponse-structure).

### Wait long enough for expiration

**Recommendation**

When calling [`getLatestBlockhash`](/api/http#getlatestblockhash) RPC API to get a recent blockhash for your transaction, take note of the `"lastValidBlockHeight"` in the response.

Then, poll the [`getBlockHeight`](/api/http#getblockheight) RPC API with the “confirmed” commitment level until it returns a block height greater than the previously returned last valid block height.

### Consider using “durable” transactions

Sometimes transaction expiration issues are really hard to avoid (e.g. offline signing, cluster instability). If the previous tips are still not sufficient for your use-case, you can switch to using durable transactions (they just require a bit of setup).

To start using durable transactions, a user first needs to submit a transaction that [invokes instructions that create a special on-chain “nonce” account](https://docs.rs/solana-program/latest/solana_program/system_instruction/fn.create_nonce_account.html) and stores a “durable blockhash” inside of it. At any point in the future (as long as the nonce account hasn’t been used yet), the user can create a durable transaction by following these 2 rules:

1. The instruction list must start with an [“advance nonce” system instruction](https://docs.rs/solana-program/latest/solana_program/system_instruction/fn.advance_nonce_account.html) which loads their on-chain nonce account
2. The transaction’s blockhash must be equal to the durable blockhash stored by the on-chain nonce account

Here’s how these transactions are processed by the Solana runtime:

1. If the transaction’s blockhash is no longer “recent”, the runtime checks if the transaction’s instruction list begins with an “advance nonce” system instruction
2. If so, it then loads the nonce account specified by the “advance nonce” instruction
3. Then it checks that the stored durable blockhash matches the transaction’s blockhash
4. Lastly it makes sure to advance the nonce account’s stored blockhash to the latest recent blockhash to ensure that the same transaction can never be processed again

For more details about how these durable transactions work, you can read the [original proposal](./../implemented-proposals/durable-tx-nonces.md) and [check out an example](./clients/javascript-reference#nonceaccount) in the Solana docs.
