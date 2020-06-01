# Add Solana to Your Exchange

This guide describes how to add Solana's native token SOL to your cryptocurrency
exchange.

## Node Setup

We highly recommend setting up at least two of your own Solana api nodes to
give you a trusted entrypoint to the network, allow you full control over how
much data is retained, and ensure you do not miss any data if one node fails.

To run an api node:

1. [Install the Solana command-line tool suite](../cli/install-solana-cli-tools.md)
2. Boot the node with at least the following parameters:
```bash
solana-validator \
  --ledger <LEDGER_PATH> \
  --entrypoint <CLUSTER_ENTRYPOINT> \
  --expected-genesis-hash <EXPECTED_GENESIS_HASH> \
  --expected-shred-version <EXPECTED_SHRED_VERSION> \
  --rpc-port 8899 \
  --no-voting \
  --enable-rpc-transaction-history \
  --limit-ledger-size <SHRED_COUNT> \
  --trusted-validator <VALIDATOR_ADDRESS> \
  --no-untrusted-rpc
```

  Customize `--ledger` to your desired ledger storage location, and `--rpc-port` to the port you want to expose.

  The `--entrypoint`, `--expected-genesis-hash`, and `--expected-shred-version` parameters are all specific to the cluster you are joining. The shred version will change on any hard forks in the cluster, so including `--expected-shred-version` ensures you are receiving current data from the cluster you expect.
  [Current parameters for Mainnet Beta](../clusters.md#example-solana-validator-command-line-2)

  The `--limit-ledger-size` parameter allows you to specify how many ledger [shreds](../terminology.md#shred) your node retains on disk. If you do not include this parameter, the ledger will keep the entire ledger until it runs out of disk space. A larger value like `--limit-ledger-size 250000000000` is good for a couple days

  Specifying one or more `--trusted-validator` parameters can protect you from booting from a malicious snapshot. [More on the value of booting with trusted validators](../running-validator/validator-start.md#trusted-validators)

  Optional parameters to consider:
  - `--private-rpc` prevents your RPC port from being published for use by other nodes
  - `--rpc-bind-address` allows you to specify a different IP address to bind the RPC port

### Automatic Restarts

We recommend configuring each of your nodes to restart automatically on exit, to
ensure you miss as little data as possible. Running the solana software as a
systemd service is one great option.

### Ledger Continuity

By default, each of your nodes will boot from a snapshot provided by one of your
trusted validators. This snapshot reflects the current state of the chain, but
does not contain the complete historical ledger. If one of your node exits and
boots from a new snapshot, there may be a gap in the ledger on that node. In
order to prevent this issue, add the `--no-snapshot-fetch` parameter to your
`solana-validator` command to receive historical ledger data instead of a
snapshot.

If you pass the `--no-snapshot-fetch` parameter on your initial boot, it will
take your node a very long time to catch up. We recommend booting from a
snapshot first, and then using the `--no-snapshot-fetch` parameter for reboots.

It is important to note that the amount of historical ledger available to your
nodes is limited to what your trusted validators retain. You will need to ensure
your nodes do not experience downtimes longer than this span, if ledger
continuity is crucial for you.

## Setting up Deposit Accounts

Solana accounts do not require any on-chain initialization; once they contain
some SOL, they exist. To set up a deposit account for your exchange, simply
generate a Solana keypair using any of our [wallet tools](../wallet-guide/cli.md).

We recommend using a unique deposit account for each of your users.

Solana accounts are charged [rent](../apps/rent.md) on creation and once per
epoch, but they can be made rent-exempt if they contain 2-years worth of rent in
SOL. In order to find the minimum rent-exempt balance for your deposit accounts,
query the
[`getMinimumBalanceForRentExemption` endpoint](../apps/jsonrpc-api.md#getminimumbalanceforrentexemption):

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc": "2.0","id":1,"method":"getMinimumBalanceForRentExemption","params":[0]}' localhost:8899

{"jsonrpc":"2.0","result":890880,"id":1}
```

### Offline Accounts

You may wish to keep the keys for one or more collection accounts offline for
greater security. If so, you will need to move SOL to hot accounts using our
[offline methods](../offline-signing/README.md).

## Listening for Deposits

When a user wants to deposit SOL into your exchange, instruct them to send a
transfer to the appropriate deposit address.

### Poll for Blocks

The easiest way to track all the deposit accounts for your exchange is to poll
for each confirmed block and inspect for addresses of interest, using the
JSON-RPC service of your Solana api node.

* To identify which blocks are available, send a [`getConfirmedBlocks` request](../apps/jsonrpc-api.md#getconfirmedblocks),
passing the last block you have already processed as the start-slot parameter:

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc": "2.0","id":1,"method":"getConfirmedBlocks","params":[5]}' localhost:8899

{"jsonrpc":"2.0","result":[5,6,8,9,11],"id":1}
```
Not every slot produces a block, so there may be gaps in the sequence of integers.

* For each block, request its contents with a [`getConfirmedBlock` request](../apps/jsonrpc-api.md#getconfirmedblock):

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc": "2.0","id":1,"method":"getConfirmedBlock","params":[5, "json"]}' localhost:8899

{
  "jsonrpc": "2.0",
  "result": {
    "blockhash": "2WcrsKSVANoe6xQHKtCcqNdUpCQPQ3vb6QTgi1dcE2oL",
    "parentSlot": 4,
    "previousBlockhash": "7ZDoGW83nXgP14vnn9XhGSaGjbuLdLWkQAoUQ7pg6qDZ",
    "rewards": [],
    "transactions": [
      {
        "meta": {
          "err": null,
          "fee": 5000,
          "postBalances": [
            2033973061360,
            218099990000,
            42000000003
          ],
          "preBalances": [
            2044973066360,
            207099990000,
            42000000003
          ],
          "status": {
            "Ok": null
          }
        },
        "transaction": {
          "message": {
            "accountKeys": [
              "Bbqg1M4YVVfbhEzwA9SpC9FhsaG83YMTYoR4a8oTDLX",
              "47Sbuv6jL7CViK9F2NMW51aQGhfdpUu7WNvKyH645Rfi",
              "11111111111111111111111111111111"
            ],
            "header": {
              "numReadonlySignedAccounts": 0,
              "numReadonlyUnsignedAccounts": 1,
              "numRequiredSignatures": 1
            },
            "instructions": [
              {
                "accounts": [
                  0,
                  1
                ],
                "data": "3Bxs3zyH82bhpB8j",
                "programIdIndex": 2
              }
            ],
            "recentBlockhash": "7GytRgrWXncJWKhzovVoP9kjfLwoiuDb3cWjpXGnmxWh"
          },
          "signatures": [
            "dhjhJp2V2ybQGVfELWM1aZy98guVVsxRCB5KhNiXFjCBMK5KEyzV8smhkVvs3xwkAug31KnpzJpiNPtcD5bG1t6"
          ]
        }
      }
    ]
  },
  "id": 1
}
```

The `preBalances` and `postBalances` fields allow you to track the balance
changes in every account without having to parse the entire transaction. They
list the starting and ending balances of each account in
[lamports](../terminology.md#lamport), indexed to the `accountKeys` list. For
example, if the deposit address if interest is
`47Sbuv6jL7CViK9F2NMW51aQGhfdpUu7WNvKyH645Rfi`, this transaction represents a
transfer of 218099990000 - 207099990000 = 11000000000 lamports = 11 SOL

If you need more information about the transaction type or other specifics, you
can request the block from RPC in binary format, and parse it using either our
[Rust SDK](https://github.com/solana-labs/solana) or
[Javascript SDK](https://github.com/solana-labs/solana-web3.js).

### Address History

You can also query the transaction history of a specific address.

* Send a [`getConfirmedSignaturesForAddress`](../apps/jsonrpc-api.md#getconfirmedsignaturesforaddress)
request to the api node, specifying a range of recent slots:

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc": "2.0","id":1,"method":"getConfirmedSignaturesForAddress","params":["6H94zdiaYfRfPfKjYLjyr2VFBg6JHXygy84r3qhc3NsC", 0, 10]}' localhost:8899

{
  "jsonrpc": "2.0",
  "result": [
    "35YGay1Lwjwgxe9zaH6APSHbt9gYQUCtBWTNL3aVwVGn9xTFw2fgds7qK5AL29mP63A9j3rh8KpN1TgSR62XCaby",
    "4bJdGN8Tt2kLWZ3Fa1dpwPSEkXWWTSszPSf1rRVsCwNjxbbUdwTeiWtmi8soA26YmwnKD4aAxNp8ci1Gjpdv4gsr",
    "dhjhJp2V2ybQGVfELWM1aZy98guVVsxRCB5KhNiXFjCBMK5KEyzV8smhkVvs3xwkAug31KnpzJpiNPtcD5bG1t6"
  ],
  "id": 1
}
```

* For each signature returned, get the transaction details by sending a
[`getConfirmedTransaction`](../apps/jsonrpc-api.md#getconfirmedtransaction) request:

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc": "2.0","id":1,"method":"getConfirmedTransaction","params":["dhjhJp2V2ybQGVfELWM1aZy98guVVsxRCB5KhNiXFjCBMK5KEyzV8smhkVvs3xwkAug31KnpzJpiNPtcD5bG1t6", "json"]}' localhost:8899

// Result
{
  "jsonrpc": "2.0",
  "result": {
    "slot": 5,
    "transaction": {
      "message": {
        "accountKeys": [
          "Bbqg1M4YVVfbhEzwA9SpC9FhsaG83YMTYoR4a8oTDLX",
          "47Sbuv6jL7CViK9F2NMW51aQGhfdpUu7WNvKyH645Rfi",
          "11111111111111111111111111111111"
        ],
        "header": {
          "numReadonlySignedAccounts": 0,
          "numReadonlyUnsignedAccounts": 1,
          "numRequiredSignatures": 1
        },
        "instructions": [
          {
            "accounts": [
              0,
              1
            ],
            "data": "3Bxs3zyH82bhpB8j",
            "programIdIndex": 2
          }
        ],
        "recentBlockhash": "7GytRgrWXncJWKhzovVoP9kjfLwoiuDb3cWjpXGnmxWh"
      },
      "signatures": [
        "dhjhJp2V2ybQGVfELWM1aZy98guVVsxRCB5KhNiXFjCBMK5KEyzV8smhkVvs3xwkAug31KnpzJpiNPtcD5bG1t6"
      ]
    },
    "meta": {
      "err": null,
      "fee": 5000,
      "postBalances": [
        2033973061360,
        218099990000,
        42000000003
      ],
      "preBalances": [
        2044973066360,
        207099990000,
        42000000003
      ],
      "status": {
        "Ok": null
      }
    }
  },
  "id": 1
}
```

## Sending Withdrawals

To accommodate a user's request to withdraw SOL, you must generate a Solana
transfer transaction, and send it to the api node to be forwarded to your
cluster.

### Synchronous

Sending a synchronous transfer to the Solana cluster allows you to easily ensure
that a transfer is successful and finalized by the cluster.

Solana's command-line tool offers a simple command, `solana transfer`, to
generate, submit, and confirm transfer transactions. By default, this method
will wait and track progress on stderr until the transaction has been finalized
by the cluster. If the transaction fails, it will report any transaction errors.

```bash
solana transfer <USER_ADDRESS> <AMOUNT> --keypair <KEYPAIR> --url http://localhost:8899
```

The [Solana Javascript SDK](https://github.com/solana-labs/solana-web3.js)
offers a similar approach for the JS ecosystem. Use the `SystemProgram` to build
a transfer transaction, and submit it using the `sendAndConfirmTransaction`
method.

### Asynchronous

For greater flexibility, you can submit withdrawal transfers asynchronously. In
these cases, it is your responsibility to verify that the transaction succeeded
and was finalized by the cluster.

**Note:** Each transaction contains a [recent blockhash](../transaction.md#blockhash-format)
to indicate its liveness. It is **critical** to wait until this blockhash
expires before retrying a withdrawal transfer that does not appear to have been
confirmed or finalized by the cluster. Otherwise, you risk a double spend. See
more on [blockhash expiration](#blockhash-expiration) below.

First, get a recent blockhash using the [`getFees` endpoint](../apps/jsonrpc-api.md#getfees)
or the CLI command:
```bash
solana fees --url http://localhost:8899
```

In the command-line tool, pass the `--no-wait` argument to send a transfer
asynchronously, and include your recent blockhash with the `--blockhash` argument:

```bash
solana transfer <USER_ADDRESS> <AMOUNT> --no-wait --blockhash <RECENT_BLOCKHASH> --keypair <KEYPAIR> --url http://localhost:8899
```

You can also build, sign, and serialize the transaction manually, and fire it off to
the cluster using the JSON-RPC [`sendTransaction` endpoint](../apps/jsonrpc-api.md#sendtransaction).

#### Transaction Confirmations & Finality

Get the status of a batch of transactions using the
[`getSignatureStatuses` JSON-RPC endpoint](../apps/jsonrpc-api.md#getsignaturestatuses).
The `confirmations` field reports how many
[confirmed blocks](../terminology.md#confirmed-block) have elapsed since the
transaction was processed. If `confirmations: null`, it is [finalized](../terminology.md#finality).

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0", "id":1, "method":"getSignatureStatuses", "params":[["5VERv8NMvzbJMEkV8xnrLkEaWRtSz9CosKDYjCJjBRnbJLgp8uirBgmQpjKhoR4tjF3ZpRzrFmBV6UjKdiSZkQUW", "5j7s6NiJS3JAkvgkoc18WVAsiSaci2pxB2A6ueCJP4tprA2TFg9wSyTLeYouxPBJEMzJinENTkpA52YStRW5Dia7"]]}' http://localhost:8899

{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 82
    },
    "value": [
      {
        "slot": 72,
        "confirmations": 10,
        "err": null,
        "status": {
          "Ok": null
        }
      },
      {
        "slot": 48,
        "confirmations": null,
        "err": null,
        "status": {
          "Ok": null
        }
      }
    ]
  },
  "id": 1
}
```

#### Blockhash Expiration

When you request a recent blockhash for your withdrawal transaction using the
[`getFees` endpoint](../apps/jsonrpc-api.md#getfees) or `solana fees`, the
response will include the `lastValidSlot`, the last slot in which the blockhash
will be valid. You can check the cluster slot with a
[`getSlot` query](../apps/jsonrpc-api.md#getslot); once the cluster slot is
greater than `lastValidSlot`, the withdrawal transaction using that blockhash
should never succeed.

You can also doublecheck whether a particular blockhash is still valid by sending a
[`getFeeCalculatorForBlockhash`](../apps/jsonrpc-api.md#getfeecalculatorforblockhash)
request with the blockhash as a parameter. If the response value is null, the
blockhash is expired, and the withdrawal transaction should never succeed.

## Testing the Integration

Be sure to test your complete workflow on Solana devnet and testnet
[clusters](../clusters.md) before moving to production on mainnet-beta. Devnet
is the most open and flexible, and ideal for initial development, while testnet
offers more realistic cluster configuration. Devnet features a token faucet, but
you will need to request some testnet SOL to get going on testnet.
