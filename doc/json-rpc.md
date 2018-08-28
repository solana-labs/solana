Solana JSON RPC API
===

Solana nodes accept HTTP requests using the [JSON-RPC 2.0](https://www.jsonrpc.org/specification) specification.

To interact with a Solana node inside a JavaScript application, use the [solana-web3.js](https://github.com/solana-labs/solana-web3.js) library, which gives a convenient interface for the RPC methods.

RPC Endpoint
---

**Default port:** 8899  
eg. http://localhost:8899, http://192.168.1.88:8899

Methods
---

* [confirmTransaction](#confirmtransaction)
* [getAddress](#getaddress)
* [getBalance](#getbalance)
* [getLastId](#getlastid)
* [getTransactionCount](#gettransactioncount)
* [requestAirdrop](#requestairdrop)
* [sendTransaction](#sendtransaction)

Request Formatting
---

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

Definitions
---

* Hash: A SHA-256 hash of a chunk of data.
* Pubkey: The public key of a Ed25519 key-pair.
* Signature: An Ed25519 signature of a chunk of data.
* Transaction: A Solana instruction signed by a client key-pair.

JSON RPC API Reference
---

### confirmTransaction
Returns a transaction receipt

##### Parameters:
* `string` - Signature of Transaction to confirm, as base-58 encoded string

##### Results:
* `boolean` - Transaction status, true if Transaction is confirmed

##### Example:
```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0", "id":1, "method":"confirmTransaction", "params":["5VERv8NMvzbJMEkV8xnrLkEaWRtSz9CosKDYjCJjBRnbJLgp8uirBgmQpjKhoR4tjF3ZpRzrFmBV6UjKdiSZkQUW"]}' http://localhost:8899

// Result
{"jsonrpc":"2.0","result":true,"id":1}
```

---

### getBalance
Returns the balance of the account of provided Pubkey

##### Parameters:
* `string` - Pubkey of account to query, as base-58 encoded string

##### Results:
* `integer` - quantity, as a signed 64-bit integer

##### Example:
```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0", "id":1, "method":"getBalance", "params":["83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri"]}' http://localhost:8899

// Result
{"jsonrpc":"2.0","result":0,"id":1}
```

---

### getLastId
Returns the last entry ID from the ledger

##### Parameters:
None

##### Results:
* `string` - the ID of last entry, a Hash as base-58 encoded string

##### Example:
```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getLastId"}' http://localhost:8899

// Result
{"jsonrpc":"2.0","result":"GH7ome3EiwEr7tu9JuTh2dpYWBJK3z69Xm1ZE3MEE6JC","id":1}
```

---

### getTransactionCount
Returns the current Transaction count from the ledger

##### Parameters:
None

##### Results:
* `integer` - count, as unsigned 64-bit integer

##### Example:
```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getTransactionCount"}' http://localhost:8899

// Result
{"jsonrpc":"2.0","result":268,"id":1}
```

---

### requestAirdrop
Requests an airdrop of tokens to a Pubkey

##### Parameters:
* `string` - Pubkey of account to receive tokens, as base-58 encoded string
* `integer` - token quantity, as a signed 64-bit integer

##### Results:
* `string` - Transaction Signature of airdrop, as base-58 encoded string

##### Example:
```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"requestAirdrop", "params":["83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri", 50]}' http://localhost:8899

// Result
{"jsonrpc":"2.0","result":"5VERv8NMvzbJMEkV8xnrLkEaWRtSz9CosKDYjCJjBRnbJLgp8uirBgmQpjKhoR4tjF3ZpRzrFmBV6UjKdiSZkQUW","id":1}
```

---

### sendTransaction
Creates new transaction

##### Parameters:
* `array` - array of octets containing a fully-signed Transaction

##### Results:
* `string` - Transaction Signature, as base-58 encoded string

##### Example:
```bash
// Request
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"sendTransaction", "params":[[61, 98, 55, 49, 15, 187, 41, 215, 176, 49, 234, 229, 228, 77, 129, 221, 239, 88, 145, 227, 81, 158, 223, 123, 14, 229, 235, 247, 191, 115, 199, 71, 121, 17, 32, 67, 63, 209, 239, 160, 161, 2, 94, 105, 48, 159, 235, 235, 93, 98, 172, 97, 63, 197, 160, 164, 192, 20, 92, 111, 57, 145, 251, 6, 40, 240, 124, 194, 149, 155, 16, 138, 31, 113, 119, 101, 212, 128, 103, 78, 191, 80, 182, 234, 216, 21, 121, 243, 35, 100, 122, 68, 47, 57, 13, 39, 0, 0, 0, 0, 50, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 50, 0, 0, 0, 0, 0, 0, 0, 40, 240, 124, 194, 149, 155, 16, 138, 31, 113, 119, 101, 212, 128, 103, 78, 191, 80, 182, 234, 216, 21, 121, 243, 35, 100, 122, 68, 47, 57, 11, 12, 106, 49, 74, 226, 201, 16, 161, 192, 28, 84, 124, 97, 190, 201, 171, 186, 6, 18, 70, 142, 89, 185, 176, 154, 115, 61, 26, 163, 77, 1, 88, 98, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]]}' http://localhost:8899

// Result
{"jsonrpc":"2.0","result":"2EBVM6cB8vAAD93Ktr6Vd8p67XPbQzCJX47MpReuiCXJAtcjaxpvWpcg9Ege1Nr5Tk3a2GFrByT7WPBjdsTycY9b","id":1}
```

---
