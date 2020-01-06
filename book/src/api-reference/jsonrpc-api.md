# JSON RPC API

Solana nodes accept HTTP requests using the [JSON-RPC 2.0](https://www.jsonrpc.org/specification) specification.

To interact with a Solana node inside a JavaScript application, use the [solana-web3.js](https://github.com/solana-labs/solana-web3.js) library, which gives a convenient interface for the RPC methods.

## RPC HTTP Endpoint

**Default port:** 8899 eg. [http://localhost:8899](http://localhost:8899), [http://192.168.1.88:8899](http://192.168.1.88:8899)

## RPC PubSub WebSocket Endpoint

**Default port:** 8900 eg. ws://localhost:8900, [http://192.168.1.88:8900](http://192.168.1.88:8900)

## Methods

* [confirmTransaction](jsonrpc-api.md#confirmtransaction)
* [getAccountInfo](jsonrpc-api.md#getaccountinfo)
* [getBalance](jsonrpc-api.md#getbalance)
* [getBlockCommitment](jsonrpc-api.md#getblockcommitment)
* [getBlockTime](jsonrpc-api.md#getblocktime)
* [getClusterNodes](jsonrpc-api.md#getclusternodes)
* [getConfirmedBlock](jsonrpc-api.md#getconfirmedblock)
* [getConfirmedBlocks](jsonrpc-api.md#getconfirmedblocks)
* [getEpochInfo](jsonrpc-api.md#getepochinfo)
* [getEpochSchedule](jsonrpc-api.md#getepochschedule)
* [getGenesisHash](jsonrpc-api.md#getgenesishash)
* [getLeaderSchedule](jsonrpc-api.md#getleaderschedule)
* [getMinimumBalanceForRentExemption](jsonrpc-api.md#getminimumbalanceforrentexemption)
* [getNumBlocksSinceSignatureConfirmation](jsonrpc-api.md#getnumblockssincesignatureconfirmation)
* [getProgramAccounts](jsonrpc-api.md#getprogramaccounts)
* [getRecentBlockhash](jsonrpc-api.md#getrecentblockhash)
* [getSignatureStatus](jsonrpc-api.md#getsignaturestatus)
* [getSlot](jsonrpc-api.md#getslot)
* [getSlotLeader](jsonrpc-api.md#getslotleader)
* [getSlotsPerSegment](jsonrpc-api.md#getslotspersegment)
* [getStorageTurn](jsonrpc-api.md#getstorageturn)
* [getStorageTurnRate](jsonrpc-api.md#getstorageturnrate)
* [getTransactionCount](jsonrpc-api.md#gettransactioncount)
* [getTotalSupply](jsonrpc-api.md#gettotalsupply)
* [getVersion](jsonrpc-api.md#getversion)
* [getVoteAccounts](jsonrpc-api.md#getvoteaccounts)
* [requestAirdrop](jsonrpc-api.md#requestairdrop)
* [sendTransaction](jsonrpc-api.md#sendtransaction)
* [startSubscriptionChannel](jsonrpc-api.md#startsubscriptionchannel)
* [Subscription Websocket](jsonrpc-api.md#subscription-websocket)
  * [accountSubscribe](jsonrpc-api.md#accountsubscribe)
  * [accountUnsubscribe](jsonrpc-api.md#accountunsubscribe)
  * [programSubscribe](jsonrpc-api.md#programsubscribe)
  * [programUnsubscribe](jsonrpc-api.md#programunsubscribe)
  * [signatureSubscribe](jsonrpc-api.md#signaturesubscribe)
  * [signatureUnsubscribe](jsonrpc-api.md#signatureunsubscribe)

## Request Formatting

To make a JSON-RPC request, send an HTTP POST request with a `Content-Type: application/json` header. The JSON request data should contain 4 fields:

* `jsonrpc`, set to `"2.0"`
* `id`, a unique client-generated identifying integer
* `method`, a string containing the method to be invoked
* `params`, a JSON array of ordered parameter values

Example using curl:

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0", "id":1, "method":"getBalance", "params":["83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri"]}' 192.168.1.88:8899
```

The response output will be a JSON object with the following fields:

* `jsonrpc`, matching the request specification
* `id`, matching the request identifier
* `result`, requested data or success confirmation

Requests can be sent in batches by sending an array of JSON-RPC request objects as the data for a single POST.

## Definitions

* Hash: A SHA-256 hash of a chunk of data.
* Pubkey: The public key of a Ed25519 key-pair.
* Signature: An Ed25519 signature of a chunk of data.
* Transaction: A Solana instruction signed by a client key-pair.

## Configuring State Commitment

Solana nodes choose which bank state to query based on a commitment requirement
set by the client. Clients may specify either:
* `{"commitment":"max"}` - the node will query the most recent bank having reached `MAX_LOCKOUT_HISTORY` confirmations
* `{"commitment":"recent"}` - the node will query its most recent bank state

The commitment parameter should be included as the last element in the `params` array:

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0", "id":1, "method":"getBalance", "params":["83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri",{"commitment":"max"}]}' 192.168.1.88:8899
```

#### Default:
If commitment configuration is not provided, the node will default to `"commitment":"max"`

Only methods that query bank state accept the commitment parameter. They are indicated in the API Reference below.

#### RpcResponse Structure
Many methods that take a commitment parameter return an RpcResponse JSON object comprised of two parts:

* `context` : An RpcResponseContext JSON structure including a `slot` field at which the operation was evaluated.
* `value` : The value returned by the operation itself.


## JSON RPC API Reference

### confirmTransaction

Returns a transaction receipt

#### Parameters:

* `string` - Signature of Transaction to confirm, as base-58 encoded string
* `object` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

* `RpcResponse<boolean>` - RpcResponse JSON object with `value` field set to Transaction status, boolean true if Transaction is confirmed

#### Example:

```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0", "id":1, "method":"confirmTransaction", "params":["5VERv8NMvzbJMEkV8xnrLkEaWRtSz9CosKDYjCJjBRnbJLgp8uirBgmQpjKhoR4tjF3ZpRzrFmBV6UjKdiSZkQUW"]}' http://localhost:8899

// Result
{"jsonrpc":"2.0","result":{"context":{"slot":1},"value":true},"id":1}
```

### getAccountInfo

Returns all information associated with the account of provided Pubkey

#### Parameters:

* `string` - Pubkey of account to query, as base-58 encoded string
* `object` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

The result value will be an RpcResponse JSON object containing an AccountInfo JSON object.

* `RpcResponse<AccountInfo>`, RpcResponse JSON object with `value` field set to AccountInfo, a JSON object containing:
* `lamports`, number of lamports assigned to this account, as a u64
* `owner`, array of 32 bytes representing the program this account has been assigned to
* `data`, array of bytes representing any data associated with the account
* `executable`, boolean indicating if the account contains a program \(and is strictly read-only\)

#### Example:

```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0", "id":1, "method":"getAccountInfo", "params":["2gVkYWexTHR5Hb2aLeQN3tnngvWzisFKXDUPrgMHpdST"]}' http://localhost:8899

// Result
{"jsonrpc":"2.0","result":{"context":{"slot":1},"value":{"executable":false,"owner":[1,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],"lamports":1,"data":[3,0,0,0,0,0,0,0,1,0,0,0,0,0,1,0,0,0,0,0,0,0.23.0,0,0,0,0,0,0,50,48,53,48,45,48,49,45,48,49,84,48,48,58,48,48,58,48,48,90,252,10,7,28,246,140,88,177,98,82,10,227,89,81,18,30,194,101,199,16,11,73,133,20,246,62,114,39,20,113,189,32,50,0,0,0,0,0,0,0,247,15,36,102,167,83,225,42,133,127,82,34,36,224,207,130,109,230,224,188,163,33,213,13,5,117,211,251,65,159,197,51,0,0,0,0,0,0]}},"id":1}
```

### getBalance

Returns the balance of the account of provided Pubkey

#### Parameters:

* `string` - Pubkey of account to query, as base-58 encoded string
* `object` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

* `RpcResponse<u64>` - RpcResponse JSON object with `value` field set to quantity

#### Example:

```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0", "id":1, "method":"getBalance", "params":["83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri"]}' http://localhost:8899

// Result
{"jsonrpc":"2.0","result":{"context":{"slot":1},"value":0},"id":1}
```

### getBlockCommitment

Returns commitment for particular block

#### Parameters:

* `u64` - block, identified by Slot

#### Results:

The result field will be an array with two fields:

* Commitment
  * `null` - Unknown block
  * `object` - BlockCommitment
    * `array` - commitment, array of u64 integers logging the amount of cluster stake in lamports that has voted on the block at each depth from 0 to `MAX_LOCKOUT_HISTORY`
* 'integer' - total active stake, in lamports, of the current epoch

#### Example:

```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getBlockCommitment","params":[5]}' http://localhost:8899

// Result
{"jsonrpc":"2.0","result":[{"commitment":[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,10,32]},42],"id":1}
```

### getBlockTime

Returns the estimated production time of a block.

Each validator reports their UTC time to the ledger on a regular interval by
intermittently adding a timestamp to a Vote for a particular block. A requested
block's time is calculated from the stake-weighted mean of the Vote timestamps
in a set of recent blocks recorded on the ledger.

Nodes that are booting from snapshot or limiting ledger size (by purging old
slots) will return null timestamps for blocks below their lowest root +
`TIMESTAMP_SLOT_RANGE`. Users interested in having this historical data must
query a node that is built from genesis and retains the entire ledger.

#### Parameters:

* `u64` - block, identified by Slot

#### Results:

* `null` - block has not yet been produced
* `i64` - estimated production time, as Unix timestamp (seconds since the Unix epoch)

#### Example:

```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getBlockTime","params":[5]}' http://localhost:8899

// Result
{"jsonrpc":"2.0","result":1574721591,"id":1}
```

### getClusterNodes

Returns information about all the nodes participating in the cluster

#### Parameters:

None

#### Results:

The result field will be an array of JSON objects, each with the following sub fields:

* `pubkey` - Node public key, as base-58 encoded string
* `gossip` - Gossip network address for the node
* `tpu` - TPU network address for the node
* `rpc` - JSON RPC network address for the node, or `null` if the JSON RPC service is not enabled

#### Example:

```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0", "id":1, "method":"getClusterNodes"}' http://localhost:8899

// Result
{"jsonrpc":"2.0","result":[{"gossip":"10.239.6.48:8001","pubkey":"9QzsJf7LPLj8GkXbYT3LFDKqsj2hHG7TA3xinJHu8epQ","rpc":"10.239.6.48:8899","tpu":"10.239.6.48:8856"}],"id":1}
```

### getConfirmedBlock

Returns identity and transaction information about a confirmed block in the ledger

#### Parameters:

* `integer` - slot, as u64 integer

#### Results:

The result field will be an object with the following fields:

* `blockhash` - the blockhash of this block
* `previousBlockhash` - the blockhash of this block's parent
* `parentSlot` - the slot index of this block's parent
* `transactions` - an array of tuples containing:
  * [Transaction](transaction-api.md) object, in JSON format
  * Transaction status object, containing:
     * `status` - Transaction status:
       * `"Ok": null` - Transaction was successful
       * `"Err": <ERR>` - Transaction failed with TransactionError  [TransactionError definitions](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L14)
     * `fee` - fee this transaction was charged, as u64 integer
     * `preBalances` - array of u64 account balances from before the transaction was processed
     * `postBalances` - array of u64 account balances after the transaction was processed

#### Example:

```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc": "2.0","id":1,"method":"getConfirmedBlock","params":[430]}' localhost:8899

// Result
{"jsonrpc":"2.0","result":{"blockhash":[165,245,120,183,32,205,89,222,249,114,229,49,250,231,149,122,156,232,181,83,238,194,157,153,7,213,180,54,177,6,25,101],"parentSlot":429,"previousBlockhash":[21,108,181,90,139,241,212,203,45,78,232,29,161,31,159,188,110,82,81,11,250,74,47,140,188,28,23,96,251,164,208,166],"transactions":[[{"message":{"accountKeys":[[5],[219,181,202,40,52,148,34,136,186,59,137,160,250,225,234,17,244,160,88,116,24,176,30,227,68,11,199,38,141,68,131,228],[233,48,179,56,91,40,254,206,53,48,196,176,119,248,158,109,121,77,11,69,108,160,128,27,228,122,146,249,53,184,68,87],[6,167,213,23,25,47,10,175,198,242,101,227,251,119,204,122,218,130,197,41,208,190,59,19,110,45,0,85,32,0,0,0],[6,167,213,23,24,199,116,201,40,86,99,152,105,29,94,182,139,94,184,163,155,75,109,92,115,85,91,33,0,0,0,0],[7,97,72,29,53,116,116,187,124,77,118,36,235,211,189,179,216,53,94,115,209,16,67,252,13,163,83,128,0,0,0,0]],"header":{"numReadonlySignedAccounts":0,"numReadonlyUnsignedAccounts":3,"numRequiredSignatures":2},"instructions":[[1],{"accounts":[[3],1,2,3],"data":[[52],2,0,0,0,1,0,0,0,0,0,0,0,173,1,0,0,0,0,0,0,86,55,9,248,142,238,135,114,103,83,247,124,67,68,163,233,55,41,59,129,64,50,110,221,234,234,27,213,205,193,219,50],"program_id_index":4}],"recentBlockhash":[21,108,181,90,139,241,212,203,45,78,232,29,161,31,159,188,110,82,81,11,250,74,47,140,188,28,23,96,251,164,208,166]},"signatures":[[2],[119,9,95,108,35,95,7,1,69,101,65,45,5,204,61,114,172,88,123,238,32,201,135,229,57,50,13,21,106,216,129,183,238,43,37,101,148,81,56,232,88,136,80,65,46,189,39,106,94,13,238,54,186,48,118,186,0,62,121,122,172,171,66,5],[78,40,77,250,10,93,6,157,48,173,100,40,251,9,7,218,7,184,43,169,76,240,254,34,235,48,41,175,119,126,75,107,106,248,45,161,119,48,174,213,57,69,111,225,245,60,148,73,124,82,53,6,203,126,120,180,111,169,89,64,29,23,237,13]]},{"fee":100000,"status":{"Ok":null},"preBalances":[499998337500,15298080,1,1,1],"postBalances":[499998237500,15298080,1,1,1]}]]},"id":1}
```

### getConfirmedBlocks

Returns a list of confirmed blocks

#### Parameters:

* `integer` - start_slot, as u64 integer
* `integer` - (optional) end_slot, as u64 integer

#### Results:

The result field will be an array of u64 integers listing confirmed blocks
between start_slot and either end_slot, if provided, or latest confirmed block,
inclusive.

#### Example:

```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc": "2.0","id":1,"method":"getConfirmedBlocks","params":[5, 10]}' localhost:8899

// Result
{"jsonrpc":"2.0","result":[5,6,7,8,9,10],"id":1}
```

### getEpochInfo

Returns information about the current epoch

#### Parameters:

* `object` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

The result field will be an object with the following fields:

* `epoch`, the current epoch
* `slotIndex`, the current slot relative to the start of the current epoch
* `slotsInEpoch`, the number of slots in this epoch

#### Example:

```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getEpochInfo"}' http://localhost:8899

// Result
{"jsonrpc":"2.0","result":{"epoch":3,"slotIndex":126,"slotsInEpoch":256},"id":1}
```

### getEpochSchedule

Returns epoch schedule information from this cluster's genesis config

#### Parameters:

None

#### Results:

The result field will be an object with the following fields:

* `slots_per_epoch`, the maximum number of slots in each epoch
* `leader_schedule_slot_offset`, the number of slots before beginning of an epoch to calculate a leader schedule for that epoch
* `warmup`, whether epochs start short and grow
* `first_normal_epoch`, first normal-length epoch, log2(slots_per_epoch) - log2(MINIMUM_SLOTS_PER_EPOCH)
* `first_normal_slot`, MINIMUM_SLOTS_PER_EPOCH * (2.pow(first_normal_epoch) - 1)

#### Example:

```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getEpochSchedule"}' http://localhost:8899

// Result
{"jsonrpc":"2.0","result":{"first_normal_epoch":8,"first_normal_slot":8160,"leader_schedule_slot_offset":8192,"slots_per_epoch":8192,"warmup":true},"id":1}
```

### getGenesisHash

Returns the genesis hash

#### Parameters:

None

#### Results:

* `string` - a Hash as base-58 encoded string

#### Example:

```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getGenesisHash"}' http://localhost:8899

// Result
{"jsonrpc":"2.0","result":"GH7ome3EiwEr7tu9JuTh2dpYWBJK3z69Xm1ZE3MEE6JC","id":1}
```

### getLeaderSchedule

Returns the leader schedule for an epoch

#### Parameters:

* `slot` - (optional) Fetch the leader schedule for the epoch that corresponds to the provided slot.  If unspecified, the leader schedule for the current epoch is fetched
* `object` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

The result field will be a dictionary of leader public keys \(as base-58 encoded
strings\) and their corresponding leader slot indices as values (indices are to
the first slot in the requested epoch)

#### Example:

```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getLeaderSchedule"}' http://localhost:8899

// Result
{"jsonrpc":"2.0","result":{"4Qkev8aNZcqFNSRhQzwyLMFSsi94jHqE8WNVTJzTP99F":[0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31,32,33,34,35,36,37,38,39,40,41,42,43,44,45,46,47,48,49,50,51,52,53,54,55,56,57,58,59,60,61,62,63]},"id":1}
```

### getMinimumBalanceForRentExemption

Returns minimum balance required to make account rent exempt.

#### Parameters:

* `u64` - account data length
* `object` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

* `u64` - minimum lamports required in account

#### Example:

```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0", "id":1, "method":"getMinimumBalanceForRentExemption", "params":[50]}' http://localhost:8899

// Result
{"jsonrpc":"2.0","result":500,"id":1}
```

### getNumBlocksSinceSignatureConfirmation

Returns the current number of blocks since signature has been confirmed.

#### Parameters:

* `string` - Signature of Transaction to confirm, as base-58 encoded string
* `object` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

* `u64` - count

#### Example:

```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0", "id":1, "method":"getNumBlocksSinceSignatureConfirmation", "params":["5VERv8NMvzbJMEkV8xnrLkEaWRtSz9CosKDYjCJjBRnbJLgp8uirBgmQpjKhoR4tjF3ZpRzrFmBV6UjKdiSZkQUW"]}' http://localhost:8899

// Result
{"jsonrpc":"2.0","result":8,"id":1}
```

### getProgramAccounts

Returns all accounts owned by the provided program Pubkey

#### Parameters:

* `string` - Pubkey of program, as base-58 encoded string
* `object` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

The result field will be an array of arrays. Each sub array will contain:

* `string` - the account Pubkey as base-58 encoded string and a JSON object, with the following sub fields:
* `lamports`, number of lamports assigned to this account, as a u64
* `owner`, array of 32 bytes representing the program this account has been assigned to
* `data`, array of bytes representing any data associated with the account
* `executable`, boolean indicating if the account contains a program \(and is strictly read-only\)

#### Example:

```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0", "id":1, "method":"getProgramAccounts", "params":["8nQwAgzN2yyUzrukXsCa3JELBYqDQrqJ3UyHiWazWxHR"]}' http://localhost:8899

// Result
{"jsonrpc":"2.0","result":[["BqGKYtAKu69ZdWEBtZHh4xgJY1BYa2YBiBReQE3pe383", {"executable":false,"owner":[50,28,250,90,221,24,94,136,147,165,253,136,1,62,196,215,225,34,222,212,99,84,202,223,245,13,149,99,149,231,91,96],"lamports":1,"data":[]], ["4Nd1mBQtrMJVYVfKf2PJy9NZUZdTAsp7D4xWLs4gDB4T", {"executable":false,"owner":[50,28,250,90,221,24,94,136,147,165,253,136,1,62,196,215,225,34,222,212,99,84,202,223,245,13,149,99,149,231,91,96],"lamports":10,"data":[]]]},"id":1}
```

### getRecentBlockhash

Returns a recent block hash from the ledger, and a fee schedule that can be used to compute the cost of submitting a transaction using it.

#### Parameters:

* `object` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

An RpcResponse containing an array consisting of a string blockhash and FeeCalculator JSON object.

* `RpcResponse<array>` - RpcResponse JSON object with `value` field set to an array including:
* `string` - a Hash as base-58 encoded string
* `FeeCalculator object` - the fee schedule for this block hash

#### Example:

```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getRecentBlockhash"}' http://localhost:8899

// Result
{"jsonrpc":"2.0","result":{"context":{"slot":1},"value":["GH7ome3EiwEr7tu9JuTh2dpYWBJK3z69Xm1ZE3MEE6JC",{"lamportsPerSignature": 0}]},"id":1}
```

### getSignatureStatus

Returns the status of a given signature. This method is similar to [confirmTransaction](jsonrpc-api.md#confirmtransaction) but provides more resolution for error events.

#### Parameters:

* `string` - Signature of Transaction to confirm, as base-58 encoded string
* `object` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

* `null` - Unknown transaction
* `object` - Transaction status:
  * `"Ok": null` - Transaction was successful
  * `"Err": <ERR>` - Transaction failed with TransactionError  [TransactionError definitions](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L14)

#### Example:

```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0", "id":1, "method":"getSignatureStatus", "params":["5VERv8NMvzbJMEkV8xnrLkEaWRtSz9CosKDYjCJjBRnbJLgp8uirBgmQpjKhoR4tjF3ZpRzrFmBV6UjKdiSZkQUW"]}' http://localhost:8899

// Result
{"jsonrpc":"2.0","result":"SignatureNotFound","id":1}
```

### getSlot

Returns the current slot the node is processing

#### Parameters:

* `object` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

* `u64` - Current slot

#### Example:

```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getSlot"}' http://localhost:8899

// Result
{"jsonrpc":"2.0","result":"1234","id":1}
```

### getSlotLeader

Returns the current slot leader

#### Parameters:

* `object` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

* `string` - Node Id as base-58 encoded string

#### Example:

```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getSlotLeader"}' http://localhost:8899

// Result
{"jsonrpc":"2.0","result":"ENvAW7JScgYq6o4zKZwewtkzzJgDzuJAFxYasvmEQdpS","id":1}
```

### getSlotsPerSegment

Returns the current storage segment size in terms of slots

#### Parameters:

* `object` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

* `u64` - Number of slots in a storage segment

#### Example:

```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getSlotsPerSegment"}' http://localhost:8899
// Result
{"jsonrpc":"2.0","result":"1024","id":1}
```

### getStorageTurn

Returns the current storage turn's blockhash and slot

#### Parameters:

None

#### Results:

An array consisting of

* `string` - a Hash as base-58 encoded string indicating the blockhash of the turn slot
* `u64` - the current storage turn slot

#### Example:

```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getStorageTurn"}' http://localhost:8899
 // Result
{"jsonrpc":"2.0","result":["GH7ome3EiwEr7tu9JuTh2dpYWBJK3z69Xm1ZE3MEE6JC", "2048"],"id":1}
```

### getStorageTurnRate

Returns the current storage turn rate in terms of slots per turn

#### Parameters:

None

#### Results:

* `u64` - Number of slots in storage turn

#### Example:

```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getStorageTurnRate"}' http://localhost:8899
 // Result
{"jsonrpc":"2.0","result":"1024","id":1}
```

### getTransactionCount

Returns the current Transaction count from the ledger

#### Parameters:

* `object` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

* `u64` - count

#### Example:

```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getTransactionCount"}' http://localhost:8899

// Result
{"jsonrpc":"2.0","result":268,"id":1}
```

### getTotalSupply

Returns the current total supply in Lamports

#### Parameters:

* `object` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

* `u64` - Total supply

#### Example:

```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getTotalSupply"}' http://localhost:8899

// Result
{"jsonrpc":"2.0","result":10126,"id":1}
```

### getVersion

Returns the current solana versions running on the node

#### Parameters:

None

#### Results:

The result field will be a JSON object with the following sub fields:

* `solana-core`, software version of solana-core

#### Example:

```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getVersion"}' http://localhost:8899
// Result
{"jsonrpc":"2.0","result":{"solana-core": "0.17.2"},"id":1}
```

### getVoteAccounts

Returns the account info and associated stake for all the voting accounts in the current bank.

#### Parameters:

* `object` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

The result field will be a JSON object of `current` and `delinquent` accounts, each containing an array of JSON objects with the following sub fields:

* `votePubkey` - Vote account public key, as base-58 encoded string
* `nodePubkey` - Node public key, as base-58 encoded string
* `activatedStake` - the stake, in lamports, delegated to this vote account and active in this epoch
* `epochVoteAccount` - bool, whether the vote account is staked for this epoch
* `commission`, percentage (0-100) of rewards payout owed to the vote account
* `lastVote` - Most recent slot voted on by this vote account

#### Example:

```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getVoteAccounts"}' http://localhost:8899

// Result
{"jsonrpc":"2.0","result":{"current":[{"commission":0,"epochVoteAccount":true,"nodePubkey":"B97CCUW3AEZFGy6uUg6zUdnNYvnVq5VG8PUtb2HayTDD","lastVote":147,"activatedStake":42,"votePubkey":"3ZT31jkAGhUaw8jsy4bTknwBMP8i4Eueh52By4zXcsVw"}],"delinquent":[{"commission":127,"epochVoteAccount":false,"nodePubkey":"6ZPxeQaDo4bkZLRsdNrCzchNQr5LN9QMc9sipXv9Kw8f","lastVote":0,"activatedStake":0,"votePubkey":"CmgCk4aMS7KW1SHX3s9K5tBJ6Yng2LBaC8MFov4wx9sm"}]},"id":1}
```

### requestAirdrop

Requests an airdrop of lamports to a Pubkey

#### Parameters:

* `string` - Pubkey of account to receive lamports, as base-58 encoded string
* `integer` - lamports, as a u64
* `object` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment) (used for retrieving blockhash and verifying airdrop success)

#### Results:

* `string` - Transaction Signature of airdrop, as base-58 encoded string

#### Example:

```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"requestAirdrop", "params":["83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri", 50]}' http://localhost:8899

// Result
{"jsonrpc":"2.0","result":"5VERv8NMvzbJMEkV8xnrLkEaWRtSz9CosKDYjCJjBRnbJLgp8uirBgmQpjKhoR4tjF3ZpRzrFmBV6UjKdiSZkQUW","id":1}
```

### sendTransaction

Creates new transaction

#### Parameters:

* `array` - array of octets containing a fully-signed Transaction

#### Results:

* `string` - Transaction Signature, as base-58 encoded string

#### Example:

```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"sendTransaction", "params":[[61, 98, 55, 49, 15, 187, 41, 215, 176, 49, 234, 229, 228, 77, 129, 221, 239, 88, 145, 227, 81, 158, 223, 123, 14, 229, 235, 247, 191, 115, 199, 71, 121, 17, 32, 67, 63, 209, 239, 160, 161, 2, 94, 105, 48, 159, 235, 235, 93, 98, 172, 97, 63, 197, 160, 164, 192, 20, 92, 111, 57, 145, 251, 6, 40, 240, 124, 194, 149, 155, 16, 138, 31, 113, 119, 101, 212, 128, 103, 78, 191, 80, 182, 234, 216, 21, 121, 243, 35, 100, 122, 68, 47, 57, 13, 39, 0, 0, 0, 0, 50, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 50, 0, 0, 0, 0, 0, 0, 0, 40, 240, 124, 194, 149, 155, 16, 138, 31, 113, 119, 101, 212, 128, 103, 78, 191, 80, 182, 234, 216, 21, 121, 243, 35, 100, 122, 68, 47, 57, 11, 12, 106, 49, 74, 226, 201, 16, 161, 192, 28, 84, 124, 97, 190, 201, 171, 186, 6, 18, 70, 142, 89, 185, 176, 154, 115, 61, 26, 163, 77, 1, 88, 98, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]]}' http://localhost:8899

// Result
{"jsonrpc":"2.0","result":"2EBVM6cB8vAAD93Ktr6Vd8p67XPbQzCJX47MpReuiCXJAtcjaxpvWpcg9Ege1Nr5Tk3a2GFrByT7WPBjdsTycY9b","id":1}
```

### Subscription Websocket

After connect to the RPC PubSub websocket at `ws://<ADDRESS>/`:

* Submit subscription requests to the websocket using the methods below
* Multiple subscriptions may be active at once
* All subscriptions take an optional `confirmations` parameter, which defines

  how many confirmed blocks the node should wait before sending a notification.

  The greater the number, the more likely the notification is to represent

  consensus across the cluster, and the less likely it is to be affected by

  forking or rollbacks. If unspecified, the default value is 0; the node will

  send a notification as soon as it witnesses the event. The maximum

  `confirmations` wait length is the cluster's `MAX_LOCKOUT_HISTORY`, which

  represents the economic finality of the chain.

### accountSubscribe

Subscribe to an account to receive notifications when the lamports or data for a given account public key changes

#### Parameters:

* `string` - account Pubkey, as base-58 encoded string
* `integer` - optional, number of confirmed blocks to wait before notification.

  Default: 0, Max: `MAX_LOCKOUT_HISTORY` \(greater integers rounded down\)

#### Results:

* `integer` - Subscription id \(needed to unsubscribe\)

#### Example:

```bash
// Request
{"jsonrpc":"2.0", "id":1, "method":"accountSubscribe", "params":["CM78CPUeXjn8o3yroDHxUtKsZZgoy4GPkPPXfouKNH12"]}

{"jsonrpc":"2.0", "id":1, "method":"accountSubscribe", "params":["CM78CPUeXjn8o3yroDHxUtKsZZgoy4GPkPPXfouKNH12", 15]}

// Result
{"jsonrpc": "2.0","result": 0,"id": 1}
```

#### Notification Format:

```bash
{"jsonrpc": "2.0","method": "accountNotification", "params": {"result": {"executable":false,"owner":[1,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],"lamports":1,"data":[3,0,0,0,0,0,0,0,1,0,0,0,0,0,1,0,0,0,0,0,0,0.23.0,0,0,0,0,0,0,50,48,53,48,45,48,49,45,48,49,84,48,48,58,48,48,58,48,48,90,252,10,7,28,246,140,88,177,98,82,10,227,89,81,18,30,194,101,199,16,11,73,133,20,246,62,114,39,20,113,189,32,50,0,0,0,0,0,0,0,247,15,36,102,167,83,225,42,133,127,82,34,36,224,207,130,109,230,224,188,163,33,213,13,5,117,211,251,65,159,197,51,0,0,0,0,0,0]},"subscription":0}}
```

### accountUnsubscribe

Unsubscribe from account change notifications

#### Parameters:

* `integer` - id of account Subscription to cancel

#### Results:

* `bool` - unsubscribe success message

#### Example:

```bash
// Request
{"jsonrpc":"2.0", "id":1, "method":"accountUnsubscribe", "params":[0]}

// Result
{"jsonrpc": "2.0","result": true,"id": 1}
```

### programSubscribe

Subscribe to a program to receive notifications when the lamports or data for a given account owned by the program changes

#### Parameters:

* `string` - program\_id Pubkey, as base-58 encoded string
* `integer` - optional, number of confirmed blocks to wait before notification.

  Default: 0, Max: `MAX_LOCKOUT_HISTORY` \(greater integers rounded down\)

#### Results:

* `integer` - Subscription id \(needed to unsubscribe\)

#### Example:

```bash
// Request
{"jsonrpc":"2.0", "id":1, "method":"programSubscribe", "params":["9gZbPtbtHrs6hEWgd6MbVY9VPFtS5Z8xKtnYwA2NynHV"]}

{"jsonrpc":"2.0", "id":1, "method":"programSubscribe", "params":["9gZbPtbtHrs6hEWgd6MbVY9VPFtS5Z8xKtnYwA2NynHV", 15]}

// Result
{"jsonrpc": "2.0","result": 0,"id": 1}
```

#### Notification Format:

* `string` - account Pubkey, as base-58 encoded string
* `object` - account info JSON object \(see [getAccountInfo](jsonrpc-api.md#getaccountinfo) for field details\)

  ```bash
  {"jsonrpc":"2.0","method":"programNotification","params":{{"result":["8Rshv2oMkPu5E4opXTRyuyBeZBqQ4S477VG26wUTFxUM",{"executable":false,"lamports":1,"owner":[129,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],"data":[1,1,1,0,0,0,0,0,0,0.23.0,0,0,0,0,0,0,50,48,49,56,45,49,50,45,50,52,84,50,51,58,53,57,58,48,48,90,235,233,39,152,15,44,117,176,41,89,100,86,45,61,2,44,251,46,212,37,35,118,163,189,247,84,27,235,178,62,55,89,0,0,0,0,50,0,0,0,0,0,0,0,235,233,39,152,15,44,117,176,41,89,100,86,45,61,2,44,251,46,212,37,35,118,163,189,247,84,27,235,178,62,45,4,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]}],"subscription":0}}
  ```

### programUnsubscribe

Unsubscribe from program-owned account change notifications

#### Parameters:

* `integer` - id of account Subscription to cancel

#### Results:

* `bool` - unsubscribe success message

#### Example:

```bash
// Request
{"jsonrpc":"2.0", "id":1, "method":"programUnsubscribe", "params":[0]}

// Result
{"jsonrpc": "2.0","result": true,"id": 1}
```

### signatureSubscribe

Subscribe to a transaction signature to receive notification when the transaction is confirmed On `signatureNotification`, the subscription is automatically cancelled

#### Parameters:

* `string` - Transaction Signature, as base-58 encoded string
* `integer` - optional, number of confirmed blocks to wait before notification.

  Default: 0, Max: `MAX_LOCKOUT_HISTORY` \(greater integers rounded down\)

#### Results:

* `integer` - subscription id \(needed to unsubscribe\)

#### Example:

```bash
// Request
{"jsonrpc":"2.0", "id":1, "method":"signatureSubscribe", "params":["2EBVM6cB8vAAD93Ktr6Vd8p67XPbQzCJX47MpReuiCXJAtcjaxpvWpcg9Ege1Nr5Tk3a2GFrByT7WPBjdsTycY9b"]}

{"jsonrpc":"2.0", "id":1, "method":"signatureSubscribe", "params":["2EBVM6cB8vAAD93Ktr6Vd8p67XPbQzCJX47MpReuiCXJAtcjaxpvWpcg9Ege1Nr5Tk3a2GFrByT7WPBjdsTycY9b", 15]}

// Result
{"jsonrpc": "2.0","result": 0,"id": 1}
```

#### Notification Format:

```bash
{"jsonrpc": "2.0","method": "signatureNotification", "params": {"result": "Confirmed","subscription":0}}
```

### signatureUnsubscribe

Unsubscribe from signature confirmation notification

#### Parameters:

* `integer` - subscription id to cancel

#### Results:

* `bool` - unsubscribe success message

#### Example:

```bash
// Request
{"jsonrpc":"2.0", "id":1, "method":"signatureUnsubscribe", "params":[0]}

// Result
{"jsonrpc": "2.0","result": true,"id": 1}
```
