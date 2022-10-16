---
title: JSON RPC HTTP Methods
displayed_sidebar: apiHttpMethodsSidebar
hide_table_of_contents: true
---

Solana nodes accept HTTP requests using the [JSON-RPC 2.0](https://www.jsonrpc.org/specification) specification.

:::info
For JavaScript applications, use the [@solana/web3.js](https://github.com/solana-labs/solana-web3.js) library as a convenient interface for the RPC methods to interact with a Solana node.

For an PubSub connection to a Solana node, use the [Websocket API](./websocket.md).
:::

## RPC HTTP Endpoint

**Default port:** 8899 e.g. [http://localhost:8899](http://localhost:8899), [http://192.168.1.88:8899](http://192.168.1.88:8899)

## Request Formatting

To make a JSON-RPC request, send an HTTP POST request with a `Content-Type: application/json` header. The JSON request data should contain 4 fields:

- `jsonrpc: <string>` - set to `"2.0"`
- `id: <number>` - a unique client-generated identifying integer
- `method: <string>` - a string containing the method to be invoked
- `params: <array>` - a JSON array of ordered parameter values

Example using curl:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getBalance",
    "params": [
      "83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri"
    ]
  }
'
```

The response output will be a JSON object with the following fields:

- `jsonrpc: <string>` - matching the request specification
- `id: <number>` - matching the request identifier
- `result: <array|number|object|string>` - requested data or success confirmation

Requests can be sent in batches by sending an array of JSON-RPC request objects as the data for a single POST.

## Definitions

- Hash: A SHA-256 hash of a chunk of data.
- Pubkey: The public key of a Ed25519 key-pair.
- Transaction: A list of Solana instructions signed by a client keypair to authorize those actions.
- Signature: An Ed25519 signature of transaction's payload data including instructions. This can be used to identify transactions.

## Configuring State Commitment

For preflight checks and transaction processing, Solana nodes choose which bank
state to query based on a commitment requirement set by the client. The
commitment describes how finalized a block is at that point in time. When
querying the ledger state, it's recommended to use lower levels of commitment
to report progress and higher levels to ensure the state will not be rolled back.

In descending order of commitment (most finalized to least finalized), clients
may specify:

- `"finalized"` - the node will query the most recent block confirmed by supermajority
  of the cluster as having reached maximum lockout, meaning the cluster has
  recognized this block as finalized
- `"confirmed"` - the node will query the most recent block that has been voted on by supermajority of the cluster.
  - It incorporates votes from gossip and replay.
  - It does not count votes on descendants of a block, only direct votes on that block.
  - This confirmation level also upholds "optimistic confirmation" guarantees in
    release 1.3 and onwards.
- `"processed"` - the node will query its most recent block. Note that the block
  may still be skipped by the cluster.

For processing many dependent transactions in series, it's recommended to use
`"confirmed"` commitment, which balances speed with rollback safety.
For total safety, it's recommended to use`"finalized"` commitment.

#### Example

The commitment parameter should be included as the last element in the `params` array:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getBalance",
    "params": [
      "83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri",
      {
        "commitment": "finalized"
      }
    ]
  }
'
```

#### Default:

If commitment configuration is not provided, the node will default to `"finalized"` commitment

Only methods that query bank state accept the commitment parameter. They are indicated in the API Reference below.

#### RpcResponse Structure

Many methods that take a commitment parameter return an RpcResponse JSON object comprised of two parts:

- `context` : An RpcResponseContext JSON structure including a `slot` field at which the operation was evaluated.
- `value` : The value returned by the operation itself.

#### Parsed Responses

Some methods support an `encoding` parameter, and can return account or
instruction data in parsed JSON format if `"encoding":"jsonParsed"` is requested
and the node has a parser for the owning program. Solana nodes currently support
JSON parsing for the following native and SPL programs:

| Program                      | Account State | Instructions |
| ---------------------------- | ------------- | ------------ |
| Address Lookup               | v1.15.0       | v1.15.0      |
| BPF Loader                   | n/a           | stable       |
| BPF Upgradeable Loader       | stable        | stable       |
| Config                       | stable        |              |
| SPL Associated Token Account | n/a           | stable       |
| SPL Memo                     | n/a           | stable       |
| SPL Token                    | stable        | stable       |
| SPL Token 2022               | stable        | stable       |
| Stake                        | stable        | stable       |
| Vote                         | stable        | stable       |

The list of account parsers can be found [here](https://github.com/solana-labs/solana/blob/master/account-decoder/src/parse_account_data.rs), and instruction parsers [here](https://github.com/solana-labs/solana/blob/master/transaction-status/src/parse_instruction.rs).

## Health Check

Although not a JSON RPC API, a `GET /health` at the RPC HTTP Endpoint provides a
health-check mechanism for use by load balancers or other network
infrastructure. This request will always return a HTTP 200 OK response with a body of
"ok", "behind" or "unknown" based on the following conditions:

1. If one or more `--known-validator` arguments are provided to `solana-validator` - "ok" is returned
   when the node has within `HEALTH_CHECK_SLOT_DISTANCE` slots of the highest
   known validator, otherwise "behind". "unknown" is returned when no slot
   information from known validators is not yet available.
2. "ok" is always returned if no known validators are provided.

## JSON RPC API Reference

import GetAccountInfo from "./methods/\_getAccountInfo.mdx"

<GetAccountInfo />

import GetBalance from "./methods/\_getBalance.mdx"

<GetBalance />

import GetBlock from "./methods/\_getBlock.mdx"

<GetBlock />

import GetBlockHeight from "./methods/\_getBlockHeight.mdx"

<GetBlockHeight />

import GetBlockProduction from "./methods/\_getBlockProduction.mdx"

<GetBlockProduction />

import GetBlockCommitment from "./methods/\_getBlockCommitment.mdx"

<GetBlockCommitment />

import GetBlocks from "./methods/\_getBlocks.mdx"

<GetBlocks />

import GetBlocksWithLimit from "./methods/\_getBlocksWithLimit.mdx"

<GetBlocksWithLimit />

import GetBlockTime from "./methods/\_getBlockTime.mdx"

<GetBlockTime />

import GetClusterNodes from "./methods/\_getClusterNodes.mdx"

<GetClusterNodes />

import GetEpochInfo from "./methods/\_getEpochInfo.mdx"

<GetEpochInfo />

import GetEpochSchedule from "./methods/\_getEpochSchedule.mdx"

<GetEpochSchedule />

import GetFeeForMessage from "./methods/\_getFeeForMessage.mdx"

<GetFeeForMessage />

import GetFirstAvailableBlock from "./methods/\_getFirstAvailableBlock.mdx"

<GetFirstAvailableBlock />

import GetGenesisHash from "./methods/\_getGenesisHash.mdx"

<GetGenesisHash />

import GetHealth from "./methods/\_getHealth.mdx"

<GetHealth />

import GetHighestSnapshotSlot from "./methods/\_getHighestSnapshotSlot.mdx"

<GetHighestSnapshotSlot />

import GetIdentity from "./methods/\_getIdentity.mdx"

<GetIdentity />

import GetInflationGovernor from "./methods/\_getInflationGovernor.mdx"

<GetInflationGovernor />

import GetInflationRate from "./methods/\_getInflationRate.mdx"

<GetInflationRate />

import GetInflationReward from "./methods/\_getInflationReward.mdx"

<GetInflationReward />

import GetLargestAccounts from "./methods/\_getLargestAccounts.mdx"

<GetLargestAccounts />

import GetLatestBlockhash from "./methods/\_getLatestBlockhash.mdx"

<GetLatestBlockhash />

import GetLeaderSchedule from "./methods/\_getLeaderSchedule.mdx"

<GetLeaderSchedule />

import GetMaxRetransmitSlot from "./methods/\_getMaxRetransmitSlot.mdx"

<GetMaxRetransmitSlot />

import GetMaxShredInsertSlot from "./methods/\_getMaxShredInsertSlot.mdx"

<GetMaxShredInsertSlot />

import GetMinimumBalanceForRentExemption from "./methods/\_getMinimumBalanceForRentExemption.mdx"

<GetMinimumBalanceForRentExemption />

import GetMultipleAccounts from "./methods/\_getMultipleAccounts.mdx"

<GetMultipleAccounts />

import GetProgramAccounts from "./methods/\_getProgramAccounts.mdx"

<GetProgramAccounts />

import GetRecentPerformanceSamples from "./methods/\_getRecentPerformanceSamples.mdx"

<GetRecentPerformanceSamples />

import GetRecentPrioritizationFees from "./methods/\_getRecentPrioritizationFees.mdx"

<GetRecentPrioritizationFees />

import GetSignaturesForAddress from "./methods/\_getSignaturesForAddress.mdx"

<GetSignaturesForAddress />

import GetSignatureStatuses from "./methods/\_getSignatureStatuses.mdx"

<GetSignatureStatuses />

import GetSlot from "./methods/\_getSlot.mdx"

<GetSlot />

import GetSlotLeader from "./methods/\_getSlotLeader.mdx"

<GetSlotLeader />

import GetSlotLeaders from "./methods/\_getSlotLeaders.mdx"

<GetSlotLeaders />

import GetStakeActivation from "./methods/\_getStakeActivation.mdx"

<GetStakeActivation />

import GetStakeMinimumDelegation from "./methods/\_getStakeMinimumDelegation.mdx"

<GetStakeMinimumDelegation />

import GetSupply from "./methods/\_getSupply.mdx"

<GetSupply />

import GetTokenAccountBalance from "./methods/\_getTokenAccountBalance.mdx"

<GetTokenAccountBalance />

import GetTokenAccountsByDelegate from "./methods/\_getTokenAccountsByDelegate.mdx"

<GetTokenAccountsByDelegate />

import GetTokenAccountsByOwner from "./methods/\_getTokenAccountsByOwner.mdx"

<GetTokenAccountsByOwner />

import GetTokenLargestAccounts from "./methods/\_getTokenLargestAccounts.mdx"

<GetTokenLargestAccounts />

import GetTokenSupply from "./methods/\_getTokenSupply.mdx"

<GetTokenSupply />

import GetTransaction from "./methods/\_getTransaction.mdx"

<GetTransaction />

import GetTransactionCount from "./methods/\_getTransactionCount.mdx"

<GetTransactionCount />

import GetVersion from "./methods/\_getVersion.mdx"

<GetVersion />

import GetVoteAccounts from "./methods/\_getVoteAccounts.mdx"

<GetVoteAccounts />

import IsBlockhashValid from "./methods/\_isBlockhashValid.mdx"

<IsBlockhashValid />

import MinimumLedgerSlot from "./methods/\_minimumLedgerSlot.mdx"

<MinimumLedgerSlot />

import RequestAirdrop from "./methods/\_requestAirdrop.mdx"

<RequestAirdrop />

import SendTransaction from "./methods/\_sendTransaction.mdx"

<SendTransaction />

import SimulateTransaction from "./methods/\_simulateTransaction.mdx"

<SimulateTransaction />

## JSON RPC API Deprecated Methods

import GetConfirmedBlock from "./deprecated/\_getConfirmedBlock.mdx"

<GetConfirmedBlock />

import GetConfirmedBlocks from "./deprecated/\_getConfirmedBlocks.mdx"

<GetConfirmedBlocks />

import GetConfirmedBlocksWithLimit from "./deprecated/\_getConfirmedBlocksWithLimit.mdx"

<GetConfirmedBlocksWithLimit />

import GetConfirmedSignaturesForAddress2 from "./deprecated/\_getConfirmedSignaturesForAddress2.mdx"

<GetConfirmedSignaturesForAddress2 />

import GetConfirmedTransaction from "./deprecated/\_getConfirmedTransaction.mdx"

<GetConfirmedTransaction />

import GetFeeCalculatorForBlockhash from "./deprecated/\_getFeeCalculatorForBlockhash.mdx"

<GetFeeCalculatorForBlockhash />

import GetFeeRateGovernor from "./deprecated/\_getFeeRateGovernor.mdx"

<GetFeeRateGovernor />

import GetFees from "./deprecated/\_getFees.mdx"

<GetFees />

import GetRecentBlockhash from "./deprecated/\_getRecentBlockhash.mdx"

<GetRecentBlockhash />

import GetSnapshotSlot from "./deprecated/\_getSnapshotSlot.mdx"

<GetSnapshotSlot />
