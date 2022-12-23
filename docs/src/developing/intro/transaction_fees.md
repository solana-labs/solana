---
title: Transaction Fees
description: "Transaction fees are the small fees paid to process instructions on the network. These fees are based on computation and an optional prioritization fee."
keywords: "instruction fee, processing fee, storage fee, low fee blockchain, gas, gwei, cheap network, affordable blockchain"
---

The small fees paid to process [instructions](./terminology.md#instruction) on the Solana blockchain are known as "_transaction fees_".

As each transaction (which contains one or more instructions) is sent through the network, it gets processed by the current leader validation-client. Once confirmed as a global state transaction, this _transaction fee_ is paid to the network to help support the economic design of the Solana blockchain.

> NOTE: Transactions fees are different from the blockchain's data storage fee called [rent](./rent.md)

### Transaction Fee Calculation

Currently, the amount of resources consumed by a transaction do not impact fees in any way. This is because the runtime imposes a small cap on the amount of resources that transaction instructions can use, not to mention that the size of transactions is limited as well. So right now, transaction fees are solely determined by the number of signatures that need to be verified in a transaction. The only limit on the number of signatures in a transaction is the max size of transaction itself. Each signature (64 bytes) in a transaction (max 1232 bytes) must reference a unique public key (32 bytes) so a single transaction could contain as many as 12 signatures (not sure why you would do that). The fee per transaction signature can be fetched with the `solana` cli:

```bash
$ solana fees
Blockhash: 8eULQbYYp67o5tGF2gxACnBCKAE39TetbYYMGTx3iBFc
Lamports per signature: 5000
Last valid block height: 94236543
```

The `solana` cli `fees` subcommand calls the `getFees` RPC API method to retrieve the above output information, so your application can call that method directly as well:

```bash
$ curl http://api.mainnet-beta.solana.com -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getFees"}
'

# RESULT (lastValidSlot removed since it's inaccurate)
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 106818885
    },
    "value": {
      "blockhash": "78e3YBCMXJBiPD1HpyVtVfFzZFPG6nUycnQcyNMSUQzB",
      "feeCalculator": {
        "lamportsPerSignature": 5000
      },
      "lastValidBlockHeight": 96137823
    }
  },
  "id": 1
}
```

### Fee Determinism

It's important to keep in mind that fee rates (such as `lamports_per_signature`) are subject to change from block to block (though that hasn't happened in the full history of the `mainnet-beta` cluster). Despite the fact that fees can fluctuate, fees for a transaction can still be calculated deterministically when creating (and before signing) a transaction. This determinism comes from the fact that fees are applied using the rates from the block whose blockhash matches the `recent_blockhash` field in a transaction. Blockhashes can only be referenced by a transaction for a few minutes before they expire.

Transactions with expired blockhashes will be ignored and dropped by the cluster, so it's important to understand how expiration actually works. Before transactions are added to a block and during block validation, [each transaction's recent blockhash is checked](https://github.com/solana-labs/solana/blob/647aa926673e3df4443d8b3d9e3f759e8ca2c44b/runtime/src/bank.rs#L3482) to ensure it hasn't expired yet. The max age of a transaction's blockhash is only 150 blocks. This means that if no slots are skipped in between, the blockhash for block 100 would be usable by transactions processed in blocks 101 to 252, inclusive (during block 101 the age of block 100 is "0" and during block 252 its age is "150"). However, it's important to remember that slots may be skipped and that age checks use "block height" _not_ "slot height". Since slots are skipped occasionally, the actual age of a blockhash can be a bit longer than 150 slots. At the time of writing, slot times are about 500ms and skip rate is about 5% so the expected lifetime of a transaction which uses the most recent blockhash is about 1min 19s.

### Fee Collection

Transactions are required to have at least one account which has signed the transaction and is writable. Writable signer accounts are serialized first in the list of transaction accounts and the first of these accounts is always used as the "fee payer".

Before any transaction instructions are processed, the fee payer account balance will be deducted to pay for transaction fees. If the fee payer balance is not sufficient to cover transaction fees, the transaction will be dropped by the cluster. If the balance was sufficient, the fees will be deducted whether the transaction is processed successfully or not. In fact, if any of the transaction instructions return an error or violate runtime restrictions, all account changes _except_ the transaction fee deduction will be rolled back.

### Fee Distribution

Transaction fees are partially burned and the remaining fees are collected by the validator that produced the block that the corresponding transactions were included in. The transaction fee burn rate was initialized as 50% when inflation rewards were enabled at the beginning of 2021 and has not changed so far. These fees incentivize a validator to process as many transactions as possible during its slots in the leader schedule. Collected fees are deposited in the validator's account (listed in the leader schedule for the current slot) after processing all of the transactions included in a block.

## Upcoming Changes

### Transaction wide compute budget

As of version 1.8 of the Solana protocol, the maximum compute budget for transactions is assessed on a per instruction basis. This has allowed for flexibility in protocol design to squeeze out more compute by splitting up operations across multiple instructions but this workaround has skewed the distribution of compute consumption across different transactions. To keep transaction fees properly priced, the maximum compute budget will instead be assessed over the entire transaction. This change is likely to be released in version 1.9 of the Solana protocol and is gated on the following feature switch:

```bash
$ ~/Workspace/solana (master branch) > cargo run --bin solana -- feature status 5ekBxc8itEnPv4NzGJtr8BVVQLNMQuLMNQQj7pHoLNZ9 --url mainnet-beta
5ekBxc8itEnPv4NzGJtr8BVVQLNMQuLMNQQj7pHoLNZ9 | inactive | transaction wide compute cap
```

This adjustment could negatively impact the usability of some protocols which have relied on the compute budget being reset for each instruction in a transaction. For this reason, this compute budget change will not be enabled until a new mechanism for increasing total transaction compute budget is added. This mechanism is described below...

### Request increased compute budget

As protocols have gotten more complex, the [default compute budget of 200,000 compute units](https://github.com/solana-labs/solana/blob/647aa926673e3df4443d8b3d9e3f759e8ca2c44b/sdk/src/compute_budget.rs#L105) has become a common pain-point for developers. Developers have gotten creative in working around this limitation by breaking up operations across multiple instructions and/or transactions. But in order to properly address this issue, a new program instruction will be added to request additional compute units from the runtime (up to a max of 1 million compute units). To request more compute, [create a `RequestUnits` instruction which invokes the new `Compute Budget` program](https://github.com/solana-labs/solana/blob/647aa926673e3df4443d8b3d9e3f759e8ca2c44b/sdk/src/compute_budget.rs#L44) and insert it at the beginning of a transaction. This new program will be released along with the transaction-wide compute budget change described above and is gated on the same feature switch. There are currently no increased transaction fees for using this feature, however that is likely to change.

Note that adding a `RequestUnits` compute budget instruction will take up 39 extra bytes in a serialized transaction. That breaks down into 32 bytes for the compute budget program id, 1 byte for program id index, 1 byte for empty ix account vec len, 1 byte for data vec len, and 4 bytes for the requested compute.

### Calculate transaction fees with RPC API

In order to simplify fee calculation for developers, a new `getFeeForMessage` RPC API is planned to be released in v1.9 of the Solana protocol. This new method accepts a blockhash along with an encoded transaction message and will return the amount of fees that would be deducted if the transaction message is signed, sent, and processed by the cluster. Full documentation can be found on the "edge" version of the Solana docs here: [https://edge.docs.solana.com/developing/clients/jsonrpc-api#getfeeformessage](https://edge.docs.solana.com/developing/clients/jsonrpc-api#getfeeformessage)
