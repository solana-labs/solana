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
* [getFeeCalculatorForBlockhash](jsonrpc-api.md#getfeecalculatorforblockhash)
* [getFeeRateGovernor](jsonrpc-api.md#getfeerategovernor)
* [getGenesisHash](jsonrpc-api.md#getgenesishash)
* [getIdentity](jsonrpc-api.md#getidentity)
* [getInflation](jsonrpc-api.md#getinflation)
* [getLeaderSchedule](jsonrpc-api.md#getleaderschedule)
* [getMinimumBalanceForRentExemption](jsonrpc-api.md#getminimumbalanceforrentexemption)
* [getProgramAccounts](jsonrpc-api.md#getprogramaccounts)
* [getRecentBlockhash](jsonrpc-api.md#getrecentblockhash)
* [getSignatureStatus](jsonrpc-api.md#getsignaturestatus)
* [getSignatureStatuses](jsonrpc-api.md#getsignaturestatuses)
* [getSlot](jsonrpc-api.md#getslot)
* [getSlotLeader](jsonrpc-api.md#getslotleader)
* [getSlotsPerSegment](jsonrpc-api.md#getslotspersegment)
* [getStoragePubkeysForSlot](jsonrpc-api.md#getstoragepubkeysforslot)
* [getStorageTurn](jsonrpc-api.md#getstorageturn)
* [getStorageTurnRate](jsonrpc-api.md#getstorageturnrate)
* [getTransactionCount](jsonrpc-api.md#gettransactioncount)
* [getTotalSupply](jsonrpc-api.md#gettotalsupply)
* [getVersion](jsonrpc-api.md#getversion)
* [getVoteAccounts](jsonrpc-api.md#getvoteaccounts)
* [minimumLedgerSlot](jsonrpc-api.md#minimumledgerslot)
* [requestAirdrop](jsonrpc-api.md#requestairdrop)
* [sendTransaction](jsonrpc-api.md#sendtransaction)
* [setLogFilter](jsonrpc-api.md#setlogfilter)
* [validatorExit](jsonrpc-api.md#validatorexit)
* [Subscription Websocket](jsonrpc-api.md#subscription-websocket)
  * [accountSubscribe](jsonrpc-api.md#accountsubscribe)
  * [accountUnsubscribe](jsonrpc-api.md#accountunsubscribe)
  * [programSubscribe](jsonrpc-api.md#programsubscribe)
  * [programUnsubscribe](jsonrpc-api.md#programunsubscribe)
  * [signatureSubscribe](jsonrpc-api.md#signaturesubscribe)
  * [signatureUnsubscribe](jsonrpc-api.md#signatureunsubscribe)
  * [slotSubscribe](jsonrpc-api.md#slotsubscribe)
  * [slotUnsubscribe](jsonrpc-api.md#slotunsubscribe)

## Request Formatting

To make a JSON-RPC request, send an HTTP POST request with a `Content-Type: application/json` header. The JSON request data should contain 4 fields:

* `jsonrpc: <string>`, set to `"2.0"`
* `id: <number>`, a unique client-generated identifying integer
* `method: <string>`, a string containing the method to be invoked
* `params: <array>`, a JSON array of ordered parameter values

Example using curl:

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0", "id":1, "method":"getBalance", "params":["83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri"]}' 192.168.1.88:8899
```

The response output will be a JSON object with the following fields:

* `jsonrpc: <string>`, matching the request specification
* `id: <number>`, matching the request identifier
* `result: <array|number|object|string>`, requested data or success confirmation

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

Returns a transaction receipt. This method only searches the recent status cache of signatures, which retains all active slots plus `MAX_RECENT_BLOCKHASHES` rooted slots.

#### Parameters:

* `<string>` - Signature of Transaction to confirm, as base-58 encoded string
* `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

* `RpcResponse<bool>` - RpcResponse JSON object with `value` field set to Transaction status, boolean true if Transaction is confirmed

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

* `<string>` - Pubkey of account to query, as base-58 encoded string
* `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

The result value will be an RpcResponse JSON object containing an AccountInfo JSON object.

* `RpcResponse<AccountInfo>`, RpcResponse JSON object with `value` field set to AccountInfo, a JSON object containing:
* `lamports: <u64>`, number of lamports assigned to this account, as a u64
* `owner: <string>`, base-58 encoded Pubkey of the program this account has been assigned to
* `data: <string>`, base-58 encoded data associated with the account
* `executable: <bool>`, boolean indicating if the account contains a program \(and is strictly read-only\)
* `rentEpoch`: <u64>, the epoch at which this account will next owe rent, as u64

#### Example:

```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0", "id":1, "method":"getAccountInfo", "params":["2gVkYWexTHR5Hb2aLeQN3tnngvWzisFKXDUPrgMHpdST"]}' http://localhost:8899

// Result
{"jsonrpc":"2.0","result":{"context":{"slot":1},"value":{"executable":false,"owner":"4uQeVj5tqViQh7yWWGStvkEG1Zmhx6uasJtWCJziofM","lamports":1,"data":"Joig2k8Ax4JPMpWhXRyc2jMa7Wejz4X1xqVi3i7QRkmVj1ChUgNc4VNpGUQePJGBAui3c6886peU9GEbjsyeANN8JGStprwLbLwcw5wpPjuQQb9mwrjVmoDQBjj3MzZKgeHn6wmnQ5k8DBFuoCYKWWsJfH2gv9FvCzrN6K1CRcQZzF","rentEpoch":2}},"id":1}
```

### getBalance

Returns the balance of the account of provided Pubkey

#### Parameters:

* `<string>` - Pubkey of account to query, as base-58 encoded string
* `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

* `RpcResponse<u64>` - RpcResponse JSON object with `value` field set to the balance

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

* `<u64>` - block, identified by Slot

#### Results:

The result field will be a JSON object containing:

* `commitment` - commitment, comprising either:
  * `<null>` - Unknown block
  * `<array>` - commitment, array of u64 integers logging the amount of cluster stake in lamports that has voted on the block at each depth from 0 to `MAX_LOCKOUT_HISTORY`
* `totalStake` - total active stake, in lamports, of the current epoch

#### Example:

```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getBlockCommitment","params":[5]}' http://localhost:8899

// Result
{"jsonrpc":"2.0","result":{"commitment":[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,10,32],"totalStake": 42},"id":1}
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

* `<u64>` - block, identified by Slot

#### Results:

* `<null>` - block has not yet been produced
* `<i64>` - estimated production time, as Unix timestamp (seconds since the Unix epoch)

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

* `pubkey: <string>` - Node public key, as base-58 encoded string
* `gossip: <string>` - Gossip network address for the node
* `tpu: <string>` - TPU network address for the node
* `rpc: <string>` - JSON RPC network address for the node, or `null` if the JSON RPC service is not enabled

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

* `<u64>` - slot, as u64 integer
* `<string>` - (optional) encoding for each returned Transaction, either "json" or "binary". If not provided, the default encoding is JSON.

#### Results:

The result field will be an object with the following fields:

* `blockhash: <string>` - the blockhash of this block, as base-58 encoded string
* `previousBlockhash: <string>` - the blockhash of this block's parent, as base-58 encoded string
* `parentSlot: <u64>` - the slot index of this block's parent
* `transactions: <array>` - an array of JSON objects containing:
  * `transaction: <object|string>` - [Transaction](#transaction-structure) object, either in JSON format or base-58 encoded binary data, depending on encoding parameter
  * `meta: <object>` - transaction status metadata object, containing `null` or:
     * `status: <object>` - Transaction status:
       * `"Ok": null` - Transaction was successful
       * `"Err": <ERR>` - Transaction failed with TransactionError  [TransactionError definitions](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L18)
     * `fee: <u64>` - fee this transaction was charged, as u64 integer
     * `preBalances: <array>` - array of u64 account balances from before the transaction was processed
     * `postBalances: <array>` - array of u64 account balances after the transaction was processed
* `rewards: <array>` - an array of JSON objects containing:
  * `pubkey: <string>` - The public key, as base-58 encoded string, of the account that received the reward
  * `lamports: <i64>`- number of reward lamports credited or debited by the account, as a i64

#### Example:

```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc": "2.0","id":1,"method":"getConfirmedBlock","params":[430, "json"]}' localhost:8899

// Result
{"jsonrpc":"2.0","result":{"blockhash":"Gp3t5bfDsJv1ovP8cB1SuRhXVuoTqDv7p3tymyubYg5","parentSlot":429,"previousBlockhash":"EFejToxii1L5aUF2NrK9dsbAEmZSNyN5nsipmZHQR1eA","transactions":[{"transaction":{"message":{"accountKeys":["6H94zdiaYfRfPfKjYLjyr2VFBg6JHXygy84r3qhc3NsC","39UAy8hsoYPywGPGdmun747omSr79zLSjqvPJN3zetoH","SysvarS1otHashes111111111111111111111111111","SysvarC1ock11111111111111111111111111111111","Vote111111111111111111111111111111111111111"],"header":{"numReadonlySignedAccounts":0,"numReadonlyUnsignedAccounts":3,"numRequiredSignatures":2},"instructions":[{"accounts":[1,2,3],"data":"29z5mr1JoRmJYQ6ynmk3pf31cGFRziAF1M3mT3L6sFXf5cKLdkEaMXMT8AqLpD4CpcupHmuMEmtZHpomrwfdZetSomNy3d","programIdIndex":4}],"recentBlockhash":"EFejToxii1L5aUF2NrK9dsbAEmZSNyN5nsipmZHQR1eA"},"signatures":["35YGay1Lwjwgxe9zaH6APSHbt9gYQUCtBWTNL3aVwVGn9xTFw2fgds7qK5AL29mP63A9j3rh8KpN1TgSR62XCaby","4vANMjSKiwEchGSXwVrQkwHnmsbKQmy9vdrsYxWdCup1bLsFzX8gKrFTSVDCZCae2dbxJB9mPNhqB2sD1vvr4sAD"]},"meta":{"fee":18000,"postBalances":[499999972500,15298080,1,1,1],"preBalances":[499999990500,15298080,1,1,1],"status":{"Ok":null}}}]},"id":1}

// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc": "2.0","id":1,"method":"getConfirmedBlock","params":[430, "binary"]}' localhost:8899

// Result
{"jsonrpc":"2.0","result":{"blockhash":"Gp3t5bfDsJv1ovP8cB1SuRhXVuoTqDv7p3tymyubYg5","parentSlot":429,"previousBlockhash":"EFejToxii1L5aUF2NrK9dsbAEmZSNyN5nsipmZHQR1eA","transactions":[{"transaction":"81UZJt4dh4Do66jDhrgkQudS8J2N6iG3jaVav7gJrqJSFY4Ug53iA9JFJZh2gxKWcaFdLJwhHx9mRdg9JwDAWB4ywiu5154CRwXV4FMdnPLg7bhxRLwhhYaLsVgMF5AyNRcTzjCVoBvqFgDU7P8VEKDEiMvD3qxzm1pLZVxDG1LTQpT3Dz4Uviv4KQbFQNuC22KupBoyHFB7Zh6KFdMqux4M9PvhoqcoJsJKwXjWpKu7xmEKnnrSbfLadkgjBmmjhW3fdTrFvnhQdTkhtdJxUL1xS9GMuJQer8YgSKNtUXB1eXZQwXU8bU2BjYkZE6Q5Xww8hu9Z4E4Mo4QsooVtHoP6BM3NKw8zjVbWfoCQqxTrwuSzrNCWCWt58C24LHecH67CTt2uXbYSviixvrYkK7A3t68BxTJcF1dXJitEPTFe2ceTkauLJqrJgnER4iUrsjr26T8YgWvpY9wkkWFSviQW6wV5RASTCUasVEcrDiaKj8EQMkgyDoe9HyKitSVg67vMWJFpUXpQobseWJUs5FTWWzmfHmFp8FZ","meta":{"fee":18000,"postBalances":[499999972500,15298080,1,1,1],"preBalances":[499999990500,15298080,1,1,1],"status":{"Ok":null}}}]},"id":1}
```

#### Transaction Structure

Transactions are quite different from those on other blockchains. Be sure to review [Anatomy of a Transaction](transaction.md) to learn about transactions on Solana.

The JSON structure of a transaction is defined as follows:

* `signatures: <array[string]>` - A list of base-58 encoded signatures applied to the transaction. The list is always of length `message.header.numRequiredSignatures`, and the signature at index `i` corresponds to the public key at index `i` in `message.account_keys`.
* `message: <object>` - Defines the content of the transaction.
  * `accountKeys: <array[string]>` - List of base-58 encoded public keys used by the transaction, including by the instructions and for signatures. The first `message.header.numRequiredSignatures` public keys must sign the transaction.
  * `header: <object>` - Details the account types and signatures required by the transaction.
    * `numRequiredSignatures: <number>` - The total number of signatures required to make the transaction valid. The signatures must match the first `numRequiredSignatures` of `message.account_keys`.
    * `numReadonlySignedAccounts: <number>` - The last `numReadonlySignedAccounts` of the signed keys are read-only accounts. Programs may process multiple transactions that load read-only accounts within a single PoH entry, but are not permitted to credit or debit lamports or modify account data. Transactions targeting the same read-write account are evaluated sequentially.
    * `numReadonlyUnsignedAccounts: <number>` - The last `numReadonlyUnsignedAccounts` of the unsigned keys are read-only accounts.
  * `recentBlockhash: <string>` - A base-58 encoded hash of a recent block in the ledger used to prevent transaction duplication and to give transactions lifetimes.
  * `instructions: <array[object]>` - List of program instructions that will be executed in sequence and committed in one atomic transaction if all succeed.
    * `programIdIndex: <number>` - Index into the `message.accountKeys` array indicating the program account that executes this instruction.
    * `accounts: <array[number]>` - List of ordered indices into the `message.accountKeys` array indicating which accounts to pass to the program.
    * `data: <string>` - The program input data encoded in a base-58 string.

### getConfirmedBlocks

Returns a list of confirmed blocks

#### Parameters:

* `<u64>` - start_slot, as u64 integer
* `<u64>` - (optional) end_slot, as u64 integer

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

* `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

The result field will be an object with the following fields:

* `absoluteSlot: <u64>`, the current slot
* `epoch: <u64>`, the current epoch
* `slotIndex: <u64>`, the current slot relative to the start of the current epoch
* `slotsInEpoch: <u64>`, the number of slots in this epoch

#### Example:

```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getEpochInfo"}' http://localhost:8899

// Result
{"jsonrpc":"2.0","result":{"absoluteSlot":166598,"epoch":27,"slotIndex":2790,"slotsInEpoch":8192},"id":1}
```

### getEpochSchedule

Returns epoch schedule information from this cluster's genesis config

#### Parameters:

None

#### Results:

The result field will be an object with the following fields:

* `slotsPerEpoch: <u64>`, the maximum number of slots in each epoch
* `leaderScheduleSlotOffset: <u64>`, the number of slots before beginning of an epoch to calculate a leader schedule for that epoch
* `warmup: <bool>`, whether epochs start short and grow
* `firstNormalEpoch: <u64>`, first normal-length epoch, log2(slotsPerEpoch) - log2(MINIMUM_SLOTS_PER_EPOCH)
* `firstNormalSlot: <u64>`, MINIMUM_SLOTS_PER_EPOCH * (2.pow(firstNormalEpoch) - 1)

#### Example:

```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getEpochSchedule"}' http://localhost:8899

// Result
{"jsonrpc":"2.0","result":{"firstNormalEpoch":8,"firstNormalSlot":8160,"leaderScheduleSlotOffset":8192,"slotsPerEpoch":8192,"warmup":true},"id":1}
```

### getFeeCalculatorForBlockhash

Returns the fee calculator associated with the query blockhash, or `null` if the blockhash has expired

#### Parameters:

* `blockhash: <string>`, query blockhash as a Base58 encoded string

#### Results:

The `result` field will be `null` if the query blockhash has expired, otherwise an `object` with the following fields:

* `feeCalculator: <object>`, `FeeCalculator` object describing the cluster fee rate at the queried blockhash

#### Example:

```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getFeeCalculatorForBlockhash", "params":["GJxqhuxcgfn5Tcj6y3f8X4FeCDd2RQ6SnEMo1AAxrPRZ"]}' http://localhost:8899

// Result
{"jsonrpc":"2.0","result":{"context":{"slot":221},"value":{"feeCalculator":{"lamportsPerSignature":5000}}},"id":1}
```

### getFeeRateGovernor

Returns the fee rate governor information from the root bank

#### Parameters:

None

#### Results:

The `result` field will be an `object` with the following fields:

* `burnPercent: <u8>`, Percentage of fees collected to be destroyed
* `maxLamportsPerSignature: <u64>`, Largest value `lamportsPerSignature` can attain for the next slot
* `minLamportsPerSignature: <u64>`, Smallest value `lamportsPerSignature` can attain for the next slot
* `targetLamportsPerSignature: <u64>`, Desired fee rate for the cluster
* `targetSignaturesPerSlot: <u64>`, Desired signature rate for the cluster

#### Example:

```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getFeeRateGovernor"}' http://localhost:8899

// Result
{"jsonrpc":"2.0","result":{"context":{"slot":54},"value":{"feeRateGovernor":{"burnPercent":50,"maxLamportsPerSignature":100000,"minLamportsPerSignature":5000,"targetLamportsPerSignature":10000,"targetSignaturesPerSlot":20000}}},"id":1}
```

### getGenesisHash

Returns the genesis hash

#### Parameters:

None

#### Results:

* `<string>` - a Hash as base-58 encoded string

#### Example:

```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getGenesisHash"}' http://localhost:8899

// Result
{"jsonrpc":"2.0","result":"GH7ome3EiwEr7tu9JuTh2dpYWBJK3z69Xm1ZE3MEE6JC","id":1}
```

### getIdentity

Returns the identity pubkey for the current node

#### Parameters:

None

#### Results:

The result field will be a JSON object with the following fields:

* `identity`, the identity pubkey of the current node \(as a base-58 encoded string\)

#### Example:

```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getIdentity"}' http://localhost:8899
// Result
{"jsonrpc":"2.0","result":{"identity": "2r1F4iWqVcb8M1DbAjQuFpebkQHY9hcVU4WuW2DJBppN"},"id":1}
```

### getInflation

Returns the inflation configuration of the cluster

#### Parameters:

None

#### Results:

The result field will be an Inflation object with the following fields:

* `initial: <f64>`, the initial inflation percentage from time 0
* `terminal: <f64>`, terminal inflation percentage
* `taper: <f64>`, rate per year at which inflation is lowered
* `foundation: <f64>`, percentage of total inflation allocated to the foundation
* `foundationTerm: <f64>`, duration of foundation pool inflation in years
* `storage: <f64>`, percentage of total inflation allocated to storage rewards

#### Example:

```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getInflation"}' http://localhost:8899

// Result
{"jsonrpc":"2.0","result":{"foundation":0.05,"foundationTerm":7.0,"initial":0.15,"storage":0.1,"taper":0.15,"terminal":0.015},"id":1}
```

### getLeaderSchedule

Returns the leader schedule for an epoch

#### Parameters:

* `<u64>` - (optional) Fetch the leader schedule for the epoch that corresponds to the provided slot.  If unspecified, the leader schedule for the current epoch is fetched
* `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

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

* `<usize>` - account data length
* `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

* `<u64>` - minimum lamports required in account

#### Example:

```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0", "id":1, "method":"getMinimumBalanceForRentExemption", "params":[50]}' http://localhost:8899

// Result
{"jsonrpc":"2.0","result":500,"id":1}
```

### getProgramAccounts

Returns all accounts owned by the provided program Pubkey

#### Parameters:

* `<string>` - Pubkey of program, as base-58 encoded string
* `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

The result field will be an array of JSON objects, which will contain:

* `pubkey: <string>` - the account Pubkey as base-58 encoded string
* `account: <object>` - a JSON object, with the following sub fields:
   * `lamports: <u64>`, number of lamports assigned to this account, as a u64
   * `owner: <string>`, base-58 encoded Pubkey of the program this account has been assigned to
   * `data: <string>`, base-58 encoded data associated with the account
   * `executable: <bool>`, boolean indicating if the account contains a program \(and is strictly read-only\)
   * `rentEpoch`: <u64>, the epoch at which this account will next owe rent, as u64

#### Example:

```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0", "id":1, "method":"getProgramAccounts", "params":["4Nd1mBQtrMJVYVfKf2PJy9NZUZdTAsp7D4xWLs4gDB4T"]}' http://localhost:8899

// Result
{"jsonrpc":"2.0","result":[{"account":{"data":"2R9jLfiAQ9bgdcw6h8s44439","executable":false,"lamports":15298080,"owner":"4Nd1mBQtrMJVYVfKf2PJy9NZUZdTAsp7D4xWLs4gDB4T","rentEpoch":28},"pubkey":"CxELquR1gPP8wHe33gZ4QxqGB3sZ9RSwsJ2KshVewkFY"}],"id":1}
```

### getRecentBlockhash

Returns a recent block hash from the ledger, and a fee schedule that can be used to compute the cost of submitting a transaction using it.

#### Parameters:

* `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

An RpcResponse containing a JSON object consisting of a string blockhash and FeeCalculator JSON object.

* `RpcResponse<object>` - RpcResponse JSON object with `value` field set to a JSON object including:
* `blockhash: <string>` - a Hash as base-58 encoded string
* `feeCalculator: <object>` - FeeCalculator object, the fee schedule for this block hash

#### Example:

```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getRecentBlockhash"}' http://localhost:8899

// Result
{"jsonrpc":"2.0","result":{"context":{"slot":1},"value":{"blockhash":"CSymwgTNX1j3E4qhKfJAUE41nBWEwXufoYryPbkde5RR","feeCalculator":{"burnPercent":50,"lamportsPerSignature":5000,"maxLamportsPerSignature":100000,"minLamportsPerSignature":5000,"targetLamportsPerSignature":10000,"targetSignaturesPerSlot":20000}}},"id":1}
```

### getSignatureStatus

Returns the status of a given signature. This method is similar to [confirmTransaction](jsonrpc-api.md#confirmtransaction) but provides more resolution for error events. This method only searches the recent status cache of signatures, which retains all active slots plus `MAX_RECENT_BLOCKHASHES` rooted slots.

#### Parameters:

* `<string>` - Signature of Transaction to confirm, as base-58 encoded string
* `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

* `<null>` - Unknown transaction
* `<object>` - Transaction status:
  * `"Ok": <null>` - Transaction was successful
  * `"Err": <ERR>` - Transaction failed with TransactionError  [TransactionError definitions](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L14)

#### Example:

```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0", "id":1, "method":"getSignatureStatus", "params":["5VERv8NMvzbJMEkV8xnrLkEaWRtSz9CosKDYjCJjBRnbJLgp8uirBgmQpjKhoR4tjF3ZpRzrFmBV6UjKdiSZkQUW"]}' http://localhost:8899

// Result
{"jsonrpc":"2.0","result":{"Ok": null},"id":1}
```

### getSignatureStatuses

Returns the statuses of a list of signatures. This method is similar to [confirmTransaction](jsonrpc-api.md#confirmtransaction) but provides more resolution for error events. This method only searches the recent status cache of signatures, which retains all active slots plus `MAX_RECENT_BLOCKHASHES` rooted slots.

#### Parameters:

* `<array>` - An array of transaction signatures to confirm, as base-58 encoded strings
* `<object>` - (optional) Extended Rpc configuration, containing the following optional fields:
  * `commitment: <string>` - [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

An RpcResponse containing a JSON object consisting of an array of TransactionStatus objects.

* `RpcResponse<object>` - RpcResponse JSON object with `value` field:

An array of:

* `<null>` - Unknown transaction
* `<object>`
  * `slot: <u64>` - The slot the transaction was processed
  * `confirmations: <usize | null>` - Number of blocks since signature confirmation, null if rooted
  * `status: <object>` - Transaction status
    * `"Ok": <null>` - Transaction was successful
    * `"Err": <ERR>` - Transaction failed with TransactionError  [TransactionError definitions](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L14)

#### Example:

```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0", "id":1, "method":"getSignatureStatus", "params":[["5VERv8NMvzbJMEkV8xnrLkEaWRtSz9CosKDYjCJjBRnbJLgp8uirBgmQpjKhoR4tjF3ZpRzrFmBV6UjKdiSZkQUW", "5j7s6NiJS3JAkvgkoc18WVAsiSaci2pxB2A6ueCJP4tprA2TFg9wSyTLeYouxPBJEMzJinENTkpA52YStRW5Dia7"]]]}' http://localhost:8899

// Result
{"jsonrpc":"2.0","result":{"context":{"slot":82},"value":[{"slot": 72, "confirmations": 10, "status": {"Ok": null}}, null]},"id":1}

// Result, first transaction rooted
{"jsonrpc":"2.0","result":{"context":{"slot":82},"value":[{"slot": 48, "confirmations": null, "status": {"Ok": null}}, null]},"id":1}
```

### getSlot

Returns the current slot the node is processing

#### Parameters:

* `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

* `<u64>` - Current slot

#### Example:

```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getSlot"}' http://localhost:8899

// Result
{"jsonrpc":"2.0","result":1234,"id":1}
```

### getSlotLeader

Returns the current slot leader

#### Parameters:

* `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

* `<string>` - Node identity Pubkey as base-58 encoded string

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

* `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

* `<u64>` - Number of slots in a storage segment

#### Example:

```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getSlotsPerSegment"}' http://localhost:8899
// Result
{"jsonrpc":"2.0","result":1024,"id":1}
```

### getStoragePubkeysForSlot

Returns the storage Pubkeys for a particular slot

#### Parameters:

None

#### Results:

An array of Pubkeys, as base-58 encoded strings

#### Example:

```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getStoragePubkeysForSlot","params":[1]}' http://localhost:8899
 // Result
{"jsonrpc":"2.0","result":["GH7ome3EiwEr7tu9JuTh2dpYWBJK3z69Xm1ZE3MEE6JC"],"id":1}
```

### getStorageTurn

Returns the current storage turn's blockhash and slot

#### Parameters:

None

#### Results:

A JSON object consisting of

* `blockhash: <string>` - a Hash as base-58 encoded string indicating the blockhash of the turn slot
* `slot: <u64>` - the current storage turn slot

#### Example:

```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getStorageTurn"}' http://localhost:8899
 // Result
{"jsonrpc":"2.0","result":{"blockhash": "GH7ome3EiwEr7tu9JuTh2dpYWBJK3z69Xm1ZE3MEE6JC", "slot": 2048},"id":1}
```

### getStorageTurnRate

Returns the current storage turn rate in terms of slots per turn

#### Parameters:

None

#### Results:

* `<u64>` - Number of slots in storage turn

#### Example:

```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getStorageTurnRate"}' http://localhost:8899
 // Result
{"jsonrpc":"2.0","result":1024,"id":1}
```

### getTransactionCount

Returns the current Transaction count from the ledger

#### Parameters:

* `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

* `<u64>` - count

#### Example:

```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getTransactionCount"}' http://localhost:8899

// Result
{"jsonrpc":"2.0","result":268,"id":1}
```

### getTotalSupply

Returns the current total supply in lamports

#### Parameters:

* `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

* `<u64>` - Total supply

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

The result field will be a JSON object with the following fields:

* `solana-core`, software version of solana-core

#### Example:

```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getVersion"}' http://localhost:8899
// Result
{"jsonrpc":"2.0","result":{"solana-core": "1.2.0"},"id":1}
```

### getVoteAccounts

Returns the account info and associated stake for all the voting accounts in the current bank.

#### Parameters:

* `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

The result field will be a JSON object of `current` and `delinquent` accounts, each containing an array of JSON objects with the following sub fields:

* `votePubkey: <string>` - Vote account public key, as base-58 encoded string
* `nodePubkey: <string>` - Node public key, as base-58 encoded string
* `activatedStake: <u64>` - the stake, in lamports, delegated to this vote account and active in this epoch
* `epochVoteAccount: <bool>` - bool, whether the vote account is staked for this epoch
* `commission: <number>`, percentage (0-100) of rewards payout owed to the vote account
* `lastVote: <u64>` - Most recent slot voted on by this vote account
* `epochCredits: <array>` - History of how many credits earned by the end of each epoch, as an array of arrays containing: `[epoch, credits, previousCredits]`

#### Example:

```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getVoteAccounts"}' http://localhost:8899

// Result
{"jsonrpc":"2.0","result":{"current":[{"commission":0,"epochVoteAccount":true,"epochCredits":[[1,64,0],[2,192,64]],"nodePubkey":"B97CCUW3AEZFGy6uUg6zUdnNYvnVq5VG8PUtb2HayTDD","lastVote":147,"activatedStake":42,"votePubkey":"3ZT31jkAGhUaw8jsy4bTknwBMP8i4Eueh52By4zXcsVw"}],"delinquent":[{"commission":127,"epochVoteAccount":false,"epochCredits":[],"nodePubkey":"6ZPxeQaDo4bkZLRsdNrCzchNQr5LN9QMc9sipXv9Kw8f","lastVote":0,"activatedStake":0,"votePubkey":"CmgCk4aMS7KW1SHX3s9K5tBJ6Yng2LBaC8MFov4wx9sm"}]},"id":1}
```

### minimumLedgerSlot

Returns the lowest slot that the node has information about in its ledger.  This
value may increase over time if the node is configured to purge older ledger data

#### Parameters:

None

#### Results:

* `u64` - Minimum ledger slot

#### Example:

```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"minimumLedgerSlot"}' http://localhost:8899

// Result
{"jsonrpc":"2.0","result":1234,"id":1}
```

### requestAirdrop

Requests an airdrop of lamports to a Pubkey

#### Parameters:

* `<string>` - Pubkey of account to receive lamports, as base-58 encoded string
* `<integer>` - lamports, as a u64
* `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment) (used for retrieving blockhash and verifying airdrop success)

#### Results:

* `<string>` - Transaction Signature of airdrop, as base-58 encoded string

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

* `<string>` - fully-signed Transaction, as base-58 encoded string

#### Results:

* `<string>` - Transaction Signature, as base-58 encoded string

#### Example:

```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"sendTransaction", "params":["3gKEMTuxvm3DKEJc4UyiyoNz1sxwdVRW2pyDDXqaCvUjGApnsazGh2y4W92zuaSSdJhBbWLYAkZokBt4N5oW27R7zCVaLLpLxvATL2GgheEh9DmmDR1P9r1ZqirVXM2fF3z5cafmc4EtwWc1UErFdCWj1qYvy4bDGMLXRYLURxaKytEEqrxz6JXj8rUHhDpjTZeFxmC6iAW3hZr6cmaAzewQCQfiEv2HfydriwHDtN95u3Y1EF6SuXxcRqox2aTjGye2Ln9zFj4XbnAtjCmkZhR"]}' http://localhost:8899

// Result
{"jsonrpc":"2.0","result":"2EBVM6cB8vAAD93Ktr6Vd8p67XPbQzCJX47MpReuiCXJAtcjaxpvWpcg9Ege1Nr5Tk3a2GFrByT7WPBjdsTycY9b","id":1}
```

### setLogFilter

Sets the log filter on the validator

#### Parameters:

* `<string>` - the new log filter to use

#### Results:

* `<null>`

#### Example:

```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"setLogFilter", "params":["solana_core=debug"]}' http://localhost:8899

// Result
{"jsonrpc":"2.0","result":null,"id":1}
```

### validatorExit

If a validator boots with RPC exit enabled (`--enable-rpc-exit` parameter), this request causes the validator to exit.

#### Parameters:

None

#### Results:

* `<bool>` - Whether the validator exit operation was successful

#### Example:

```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"validatorExit"}' http://localhost:8899

// Result
{"jsonrpc":"2.0","result":true,"id":1}
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

* `<string>` - account Pubkey, as base-58 encoded string
* `<u64>` - optional, number of confirmed blocks to wait before notification.

  Default: 0, Max: `MAX_LOCKOUT_HISTORY` \(greater integers rounded down\)

#### Results:

* `<number>` - Subscription id \(needed to unsubscribe\)

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
{"jsonrpc": "2.0","method": "accountNotification", "params": {"result": {"executable":false,"owner":"4uQeVj5tqViQh7yWWGStvkEG1Zmhx6uasJtWCJziofM","lamports":1,"data":"Joig2k8Ax4JPMpWhXRyc2jMa7Wejz4X1xqVi3i7QRkmVj1ChUgNc4VNpGUQePJGBAui3c6886peU9GEbjsyeANN8JGStprwLbLwcw5wpPjuQQb9mwrjVmoDQBjj3MzZKgeHn6wmnQ5k8DBFuoCYKWWsJfH2gv9FvCzrN6K1CRcQZzF","rentEpoch":28},"subscription":0}}
```

### accountUnsubscribe

Unsubscribe from account change notifications

#### Parameters:

* `<number>` - id of account Subscription to cancel

#### Results:

* `<bool>` - unsubscribe success message

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

* `<string>` - program\_id Pubkey, as base-58 encoded string
* `<u64>` - optional, number of confirmed blocks to wait before notification.

  Default: 0, Max: `MAX_LOCKOUT_HISTORY` \(greater integers rounded down\)

#### Results:

* `<integer>` - Subscription id \(needed to unsubscribe\)

#### Example:

```bash
// Request
{"jsonrpc":"2.0", "id":1, "method":"programSubscribe", "params":["9gZbPtbtHrs6hEWgd6MbVY9VPFtS5Z8xKtnYwA2NynHV"]}

{"jsonrpc":"2.0", "id":1, "method":"programSubscribe", "params":["9gZbPtbtHrs6hEWgd6MbVY9VPFtS5Z8xKtnYwA2NynHV", 15]}

// Result
{"jsonrpc": "2.0","result": 0,"id": 1}
```

#### Notification Format:

* `<string>` - account Pubkey, as base-58 encoded string
* `<object>` - account info JSON object \(see [getAccountInfo](jsonrpc-api.md#getaccountinfo) for field details\)

  ```bash
  {"jsonrpc":"2.0","method":"programNotification","params":{{"result":["8Rshv2oMkPu5E4opXTRyuyBeZBqQ4S477VG26wUTFxUM",{"executable":false,"lamports":1,"owner":"9gZbPtbtHrs6hEWgd6MbVY9VPFtS5Z8xKtnYwA2NynHV","data":"4SZWhnbSt3njU4QHVgPrWeekz1BudU4ttmdr9ezmrL4X6XeLeL83xVAo6ZdxwU3oXgHNeF2q6tWZbnVnBXmvNyeLVEGt8ZQ4ZmgjHfVNCEwBtzh2aDrHgQSjBFLYAdmM3uwBhcm1EyHJLeUiFqpsoAUhn6Vphwrpf44dWRAGsAJZbzvVrUW9bfucpR7xudHHg2MxQ2CdqsfS3TfWUJY3vaf2A4AUNzfAmNPHBGi99nU2hYubGSVSPcpVPpdRWQkydgqasBmTosd","rentEpoch":28}],"subscription":0}}
  ```

### programUnsubscribe

Unsubscribe from program-owned account change notifications

#### Parameters:

* `<integer>` - id of account Subscription to cancel

#### Results:

* `<bool>` - unsubscribe success message

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

* `<string>` - Transaction Signature, as base-58 encoded string
* `<integer>` - optional, number of confirmed blocks to wait before notification.

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

* `<integer>` - subscription id to cancel

#### Results:

* `<bool>` - unsubscribe success message

#### Example:

```bash
// Request
{"jsonrpc":"2.0", "id":1, "method":"signatureUnsubscribe", "params":[0]}

// Result
{"jsonrpc": "2.0","result": true,"id": 1}
```

### slotSubscribe

Subscribe to receive notification anytime a slot is processed by the validator

#### Parameters:

None

#### Results:

* `integer` - subscription id \(needed to unsubscribe\)

#### Example:

```bash
// Request
{"jsonrpc":"2.0", "id":1, "method":"slotSubscribe"}

// Result
{"jsonrpc": "2.0","result": 0,"id": 1}
```

#### Notification Format:

```bash
{"jsonrpc": "2.0","method": "slotNotification", "params": {"result":{"parent":75,"root":44,"slot":76},"subscription":0}}
```

### slotUnsubscribe

Unsubscribe from slot notifications

#### Parameters:

* `<integer>` - subscription id to cancel

#### Results:

* `<bool>` - unsubscribe success message

#### Example:

```bash
// Request
{"jsonrpc":"2.0", "id":1, "method":"slotUnsubscribe", "params":[0]}

// Result
{"jsonrpc": "2.0","result": true,"id": 1}
```

### rootSubscribe

Subscribe to receive notification anytime a new root is set by the validator.

#### Parameters:

None

#### Results:

* `integer` - subscription id \(needed to unsubscribe\)

#### Example:

```bash
// Request
{"jsonrpc":"2.0", "id":1, "method":"rootSubscribe"}

// Result
{"jsonrpc": "2.0","result": 0,"id": 1}
```

#### Notification Format:

The result is the latest root slot number.

```bash
{"jsonrpc": "2.0","method": "rootNotification", "params": {"result":42,"subscription":0}}
```

### rootUnsubscribe

Unsubscribe from root notifications

#### Parameters:

* `<integer>` - subscription id to cancel

#### Results:

* `<bool>` - unsubscribe success message

#### Example:

```bash
// Request
{"jsonrpc":"2.0", "id":1, "method":"rootUnsubscribe", "params":[0]}

// Result
{"jsonrpc": "2.0","result": true,"id": 1}
```
