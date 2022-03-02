---
title: Add Solana to Your Exchange
---

This guide describes how to add Solana's native token SOL to your cryptocurrency
exchange.

## Node Setup

We highly recommend setting up at least two nodes on high-grade computers/cloud
instances, upgrading to newer versions promptly, and keeping an eye on service
operations with a bundled monitoring tool.

This setup enables you:

- to have a self-administered gateway to the Solana mainnet-beta cluster to get
  data and submit withdrawal transactions
- to have full control over how much historical block data is retained
- to maintain your service availability even if one node fails

Solana nodes demand relatively high computing power to handle our fast blocks
and high TPS. For specific requirements, please see
[hardware recommendations](../running-validator/validator-reqs.md).

To run an api node:

1. [Install the Solana command-line tool suite](../cli/install-solana-cli-tools.md)
2. Start the validator with at least the following parameters:

```bash
solana-validator \
  --ledger <LEDGER_PATH> \
  --identity <VALIDATOR_IDENTITY_KEYPAIR> \
  --entrypoint <CLUSTER_ENTRYPOINT> \
  --expected-genesis-hash <EXPECTED_GENESIS_HASH> \
  --rpc-port 8899 \
  --no-voting \
  --enable-rpc-transaction-history \
  --limit-ledger-size \
  --known-validator <VALIDATOR_ADDRESS> \
  --only-known-rpc
```

Customize `--ledger` to your desired ledger storage location, and `--rpc-port` to the port you want to expose.

The `--entrypoint` and `--expected-genesis-hash` parameters are all specific to the cluster you are joining.
[Current parameters for Mainnet Beta](../clusters.md#example-solana-validator-command-line-2)

The `--limit-ledger-size` parameter allows you to specify how many ledger
[shreds](../terminology.md#shred) your node retains on disk. If you do not
include this parameter, the validator will keep the entire ledger until it runs
out of disk space. The default value attempts to keep the ledger disk usage
under 500GB. More or less disk usage may be requested by adding an argument to
`--limit-ledger-size` if desired. Check `solana-validator --help` for the
default limit value used by `--limit-ledger-size`. More information about
selecting a custom limit value is [available
here](https://github.com/solana-labs/solana/blob/583cec922b6107e0f85c7e14cb5e642bc7dfb340/core/src/ledger_cleanup_service.rs#L15-L26).

Specifying one or more `--known-validator` parameters can protect you from booting from a malicious snapshot. [More on the value of booting with known validators](../running-validator/validator-start.md#known-validators)

Optional parameters to consider:

- `--private-rpc` prevents your RPC port from being published for use by other nodes
- `--rpc-bind-address` allows you to specify a different IP address to bind the RPC port

### Automatic Restarts and Monitoring

We recommend configuring each of your nodes to restart automatically on exit, to
ensure you miss as little data as possible. Running the solana software as a
systemd service is one great option.

For monitoring, we provide
[`solana-watchtower`](https://github.com/solana-labs/solana/blob/master/watchtower/README.md),
which can monitor your validator and detect with the `solana-validator` process
is unhealthy. It can directly be configured to alert you via Slack, Telegram,
Discord, or Twillio. For details, run `solana-watchtower --help`.

```bash
solana-watchtower --validator-identity <YOUR VALIDATOR IDENTITY>
```

#### New Software Release Announcements

We release new software frequently (around 1 release / week).
Sometimes newer versions include incompatible protocol changes, which
necessitate timely software update to avoid errors in processing blocks.

Our official release announcements for all kinds of releases (normal and
security) are communicated via a discord channel called
[`#mb-announcement`](https://discord.com/channels/428295358100013066/669406841830244375)
(`mb` stands for `mainnet-beta`).

Like staked validators, we expect any exchange-operated validators to be updated
at your earliest convenience within a business day or two after a normal release
announcement. For security-related releases, more urgent action may be needed.

### Ledger Continuity

By default, each of your nodes will boot from a snapshot provided by one of your
known validators. This snapshot reflects the current state of the chain, but
does not contain the complete historical ledger. If one of your node exits and
boots from a new snapshot, there may be a gap in the ledger on that node. In
order to prevent this issue, add the `--no-snapshot-fetch` parameter to your
`solana-validator` command to receive historical ledger data instead of a
snapshot.

Do not pass the `--no-snapshot-fetch` parameter on your initial boot as it's not
possible to boot the node all the way from the genesis block. Instead boot from
a snapshot first and then add the `--no-snapshot-fetch` parameter for reboots.

It is important to note that the amount of historical ledger available to your
nodes from the rest of the network is limited at any point in time. Once
operational if your validators experience significant downtime they may not be
able to catch up to the network and will need to download a new snapshot from a
known validator. In doing so your validators will now have a gap in its
historical ledger data that cannot be filled.

### Minimizing Validator Port Exposure

The validator requires that various UDP and TCP ports be open for inbound
traffic from all other Solana validators. While this is the most efficient mode of
operation, and is strongly recommended, it is possible to restrict the
validator to only require inbound traffic from one other Solana validator.

First add the `--restricted-repair-only-mode` argument. This will cause the
validator to operate in a restricted mode where it will not receive pushes from
the rest of the validators, and instead will need to continually poll other
validators for blocks. The validator will only transmit UDP packets to other
validators using the _Gossip_ and _ServeR_ ("serve repair") ports, and only
receive UDP packets on its _Gossip_ and _Repair_ ports.

The _Gossip_ port is bi-directional and allows your validator to remain in
contact with the rest of the cluster. Your validator transmits on the _ServeR_
to make repair requests to obtaining new blocks from the rest of the network,
since Turbine is now disabled. Your validator will then receive repair
responses on the _Repair_ port from other validators.

To further restrict the validator to only requesting blocks from one or more
validators, first determine the identity pubkey for that validator and add the
`--gossip-pull-validator PUBKEY --repair-validator PUBKEY` arguments for each
PUBKEY. This will cause your validator to be a resource drain on each validator
that you add, so please do this sparingly and only after consulting with the
target validator.

Your validator should now only be communicating with the explicitly listed
validators and only on the _Gossip_, _Repair_ and _ServeR_ ports.

## Setting up Deposit Accounts

Solana accounts do not require any on-chain initialization; once they contain
some SOL, they exist. To set up a deposit account for your exchange, simply
generate a Solana keypair using any of our [wallet tools](../wallet-guide/cli.md).

We recommend using a unique deposit account for each of your users.

Solana accounts must be made rent-exempt by containing 2-years worth of
[rent](developing/programming-model/accounts.md#rent) in SOL. In order to find
the minimum rent-exempt balance for your deposit accounts, query the
[`getMinimumBalanceForRentExemption` endpoint](developing/clients/jsonrpc-api.md#getminimumbalanceforrentexemption):

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc": "2.0","id":1,"method":"getMinimumBalanceForRentExemption","params":[0]}' localhost:8899

{"jsonrpc":"2.0","result":890880,"id":1}
```

### Offline Accounts

You may wish to keep the keys for one or more collection accounts offline for
greater security. If so, you will need to move SOL to hot accounts using our
[offline methods](../offline-signing.md).

## Listening for Deposits

When a user wants to deposit SOL into your exchange, instruct them to send a
transfer to the appropriate deposit address.

### Poll for Blocks

To track all the deposit accounts for your exchange, poll for each confirmed
block and inspect for addresses of interest, using the JSON-RPC service of your
Solana API node.

- To identify which blocks are available, send a [`getBlocks` request](developing/clients/jsonrpc-api.md#getblocks),
  passing the last block you have already processed as the start-slot parameter:

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc": "2.0","id":1,"method":"getBlocks","params":[5]}' localhost:8899

{"jsonrpc":"2.0","result":[5,6,8,9,11],"id":1}
```

Not every slot produces a block, so there may be gaps in the sequence of integers.

- For each block, request its contents with a [`getBlock` request](developing/clients/jsonrpc-api.md#getblock):

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc": "2.0","id":1,"method":"getBlock","params":[5, "json"]}' localhost:8899

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

You can also query the transaction history of a specific address. This is
generally _not_ a viable method for tracking all your deposit addresses over all
slots, but may be useful for examining a few accounts for a specific period of
time.

- Send a [`getSignaturesForAddress`](developing/clients/jsonrpc-api.md#getsignaturesforaddress)
  request to the api node:

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc": "2.0","id":1,"method":"getSignaturesForAddress","params":["6H94zdiaYfRfPfKjYLjyr2VFBg6JHXygy84r3qhc3NsC", {"limit": 3}]}' localhost:8899

{
  "jsonrpc": "2.0",
  "result": [
    {
      "err": null,
      "memo": null,
      "signature": "35YGay1Lwjwgxe9zaH6APSHbt9gYQUCtBWTNL3aVwVGn9xTFw2fgds7qK5AL29mP63A9j3rh8KpN1TgSR62XCaby",
      "slot": 114
    },
    {
      "err": null,
      "memo": null,
      "signature": "4bJdGN8Tt2kLWZ3Fa1dpwPSEkXWWTSszPSf1rRVsCwNjxbbUdwTeiWtmi8soA26YmwnKD4aAxNp8ci1Gjpdv4gsr",
      "slot": 112
    },
    {
      "err": null,
      "memo": null,
      "signature": "dhjhJp2V2ybQGVfELWM1aZy98guVVsxRCB5KhNiXFjCBMK5KEyzV8smhkVvs3xwkAug31KnpzJpiNPtcD5bG1t6",
      "slot": 108
    }
  ],
  "id": 1
}
```

- For each signature returned, get the transaction details by sending a
  [`getTransaction`](developing/clients/jsonrpc-api.md#gettransaction) request:

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc": "2.0","id":1,"method":"getTransaction","params":["dhjhJp2V2ybQGVfELWM1aZy98guVVsxRCB5KhNiXFjCBMK5KEyzV8smhkVvs3xwkAug31KnpzJpiNPtcD5bG1t6", "json"]}' localhost:8899

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
solana transfer <USER_ADDRESS> <AMOUNT> --allow-unfunded-recipient --keypair <KEYPAIR> --url http://localhost:8899
```

The [Solana Javascript SDK](https://github.com/solana-labs/solana-web3.js)
offers a similar approach for the JS ecosystem. Use the `SystemProgram` to build
a transfer transaction, and submit it using the `sendAndConfirmTransaction`
method.

### Asynchronous

For greater flexibility, you can submit withdrawal transfers asynchronously. In
these cases, it is your responsibility to verify that the transaction succeeded
and was finalized by the cluster.

**Note:** Each transaction contains a [recent
blockhash](developing/programming-model/transactions.md#blockhash-format) to
indicate its liveness. It is **critical** to wait until this blockhash expires
before retrying a withdrawal transfer that does not appear to have been
confirmed or finalized by the cluster. Otherwise, you risk a double spend. See
more on [blockhash expiration](#blockhash-expiration) below.

First, get a recent blockhash using the [`getFees` endpoint](developing/clients/jsonrpc-api.md#getfees)
or the CLI command:

```bash
solana fees --url http://localhost:8899
```

In the command-line tool, pass the `--no-wait` argument to send a transfer
asynchronously, and include your recent blockhash with the `--blockhash` argument:

```bash
solana transfer <USER_ADDRESS> <AMOUNT> --no-wait --allow-unfunded-recipient --blockhash <RECENT_BLOCKHASH> --keypair <KEYPAIR> --url http://localhost:8899
```

You can also build, sign, and serialize the transaction manually, and fire it off to
the cluster using the JSON-RPC [`sendTransaction` endpoint](developing/clients/jsonrpc-api.md#sendtransaction).

#### Transaction Confirmations & Finality

Get the status of a batch of transactions using the
[`getSignatureStatuses` JSON-RPC endpoint](developing/clients/jsonrpc-api.md#getsignaturestatuses).
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

You can check whether a particular blockhash is still valid by sending a
[`getFeeCalculatorForBlockhash`](developing/clients/jsonrpc-api.md#getfeecalculatorforblockhash)
request with the blockhash as a parameter. If the response value is `null`, the
blockhash is expired, and the withdrawal transaction using that blockhash should
never succeed.

### Validating User-supplied Account Addresses for Withdrawals

As withdrawals are irreversible, it may be a good practice to validate a
user-supplied account address before authorizing a withdrawal in order to
prevent accidental loss of user funds.

#### Basic verfication

Solana addresses a 32-byte array, encoded with the bitcoin base58 alphabet. This
results in an ASCII text string matching the following regular expression:

```
[1-9A-HJ-NP-Za-km-z]{32,44}
```

This check is insufficient on its own as Solana addresses are not checksummed, so
typos cannot be detected. To further validate the user's input, the string can be
decoded and the resulting byte array's length confirmed to be 32. However, there
are some addresses that can decode to 32 bytes despite a typo such as a single
missing character, reversed characters and ignored case

#### Advanced verification

Due to the vulnerability to typos described above, it is recommended that the
balance be queried for candidate withdraw addresses and the user prompted to
confirm their intentions if a non-zero balance is discovered.

#### Valid ed25519 pubkey check

The address of a normal account in Solana is a Base58-encoded string of a
256-bit ed25519 public key. Not all bit patterns are valid public keys for the
ed25519 curve, so it is possible to ensure user-supplied account addresses are
at least correct ed25519 public keys.

#### Java

Here is a Java example of validating a user-supplied address as a valid ed25519
public key:

The following code sample assumes you're using the Maven.

`pom.xml`:

```xml
<repositories>
  ...
  <repository>
    <id>spring</id>
    <url>https://repo.spring.io/libs-release/</url>
  </repository>
</repositories>

...

<dependencies>
  ...
  <dependency>
      <groupId>io.github.novacrypto</groupId>
      <artifactId>Base58</artifactId>
      <version>0.1.3</version>
  </dependency>
  <dependency>
      <groupId>cafe.cryptography</groupId>
      <artifactId>curve25519-elisabeth</artifactId>
      <version>0.1.0</version>
  </dependency>
<dependencies>
```

```java
import io.github.novacrypto.base58.Base58;
import cafe.cryptography.curve25519.CompressedEdwardsY;

public class PubkeyValidator
{
    public static boolean verifyPubkey(String userProvidedPubkey)
    {
        try {
            return _verifyPubkeyInternal(userProvidedPubkey);
        } catch (Exception e) {
            return false;
        }
    }

    public static boolean _verifyPubkeyInternal(String maybePubkey) throws Exception
    {
        byte[] bytes = Base58.base58Decode(maybePubkey);
        return !(new CompressedEdwardsY(bytes)).decompress().isSmallOrder();
    }
}
```

## Minimum Deposit & Withdrawal Amounts

Every deposit and withdrawal of SOL must be greater or equal to the minimum
rent-exempt balance for the account at the wallet address (a basic SOL account
holding no data), currently: 0.000890880 SOL

Similarly, every deposit account must contain at least this balance.

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc": "2.0","id":1,"method":"getMinimumBalanceForRentExemption","params":[0]}' localhost:8899

{"jsonrpc":"2.0","result":890880,"id":1}
```

## Supporting the SPL Token Standard

[SPL Token](https://spl.solana.com/token) is the standard for wrapped/synthetic
token creation and exchange on the Solana blockchain.

The SPL Token workflow is similar to that of native SOL tokens, but there are a
few differences which will be discussed in this section.

### Token Mints

Each _type_ of SPL Token is declared by creating a _mint_ account. This account
stores metadata describing token features like the supply, number of decimals, and
various authorities with control over the mint. Each SPL Token account references
its associated mint and may only interact with SPL Tokens of that type.

### Installing the `spl-token` CLI Tool

SPL Token accounts are queried and modified using the `spl-token` command line
utility. The examples provided in this section depend upon having it installed
on the local system.

`spl-token` is distributed from Rust [crates.io](https://crates.io/crates/spl-token)
via the Rust `cargo` command line utility. The latest version of `cargo` can be
installed using a handy one-liner for your platform at [rustup.rs](https://rustup.rs).
Once `cargo` is installed, `spl-token` can be obtained with the following command:

```
cargo install spl-token-cli
```

You can then check the installed version to verify

```
spl-token --version
```

Which should result in something like

```text
spl-token-cli 2.0.1
```

### Account Creation

SPL Token accounts carry additional requirements that native System Program
accounts do not:

1. SPL Token accounts must be created before an amount of tokens can be
   deposited. Token accounts can be created explicitly with the
   `spl-token create-account` command, or implicitly by the
   `spl-token transfer --fund-recipient ...` command.
1. SPL Token accounts must remain [rent-exempt](developing/programming-model/accounts.md#rent-exemption)
   for the duration of their existence and therefore require a small amount of
   native SOL tokens be deposited at account creation. For SPL Token v2 accounts,
   this amount is 0.00203928 SOL (2,039,280 lamports).

#### Command Line

To create an SPL Token account with the following properties:

1. Associated with the given mint
1. Owned by the funding account's keypair

```
spl-token create-account <TOKEN_MINT_ADDRESS>
```

#### Example

```
$ spl-token create-account AkUFCWTXb3w9nY2n6SFJvBV6VwvFUCe4KBMCcgLsa2ir
Creating account 6VzWGL51jLebvnDifvcuEDec17sK6Wupi4gYhm5RzfkV
Signature: 4JsqZEPra2eDTHtHpB4FMWSfk3UgcCVmkKkP7zESZeMrKmFFkDkNd91pKP3vPVVZZPiu5XxyJwS73Vi5WsZL88D7
```

Or to create an SPL Token account with a specific keypair:

```
$ solana-keygen new -o token-account.json
$ spl-token create-account AkUFCWTXb3w9nY2n6SFJvBV6VwvFUCe4KBMCcgLsa2ir token-account.json
Creating account 6VzWGL51jLebvnDifvcuEDec17sK6Wupi4gYhm5RzfkV
Signature: 4JsqZEPra2eDTHtHpB4FMWSfk3UgcCVmkKkP7zESZeMrKmFFkDkNd91pKP3vPVVZZPiu5XxyJwS73Vi5WsZL88D7
```

### Checking an Account's Balance

#### Command Line

```
spl-token balance <TOKEN_ACCOUNT_ADDRESS>
```

#### Example

```
$ solana balance 6VzWGL51jLebvnDifvcuEDec17sK6Wupi4gYhm5RzfkV
0
```

### Token Transfers

The source account for a transfer is the actual token account that contains the
amount.

The recipient address however can be a normal wallet account. If an associated
token account for the given mint does not yet exist for that wallet, the
transfer will create it provided that the `--fund-recipient` argument as
provided.

#### Command Line

```
spl-token transfer <SENDER_ACCOUNT_ADDRESS> <AMOUNT> <RECIPIENT_WALLET_ADDRESS> --fund-recipient
```

#### Example

```
$ spl-token transfer 6B199xxzw3PkAm25hGJpjj3Wj3WNYNHzDAnt1tEqg5BN 1 6VzWGL51jLebvnDifvcuEDec17sK6Wupi4gYhm5RzfkV
Transfer 1 tokens
  Sender: 6B199xxzw3PkAm25hGJpjj3Wj3WNYNHzDAnt1tEqg5BN
  Recipient: 6VzWGL51jLebvnDifvcuEDec17sK6Wupi4gYhm5RzfkV
Signature: 3R6tsog17QM8KfzbcbdP4aoMfwgo6hBggJDVy7dZPVmH2xbCWjEj31JKD53NzMrf25ChFjY7Uv2dfCDq4mGFFyAj
```

### Depositing

Since each `(wallet, mint)` pair requires a separate account on chain. It is
recommended that the addresses for these accounts be derived from SOL deposit
wallets using the
[Associated Token Account](https://spl.solana.com/associated-token-account) (ATA)
scheme and that _only_ deposits from ATA addresses be accepted.

Monitoring for deposit transactions should follow the [block polling](#poll-for-blocks)
method described above. Each new block should be scanned for successful
transactions referencing user token-account derived addresses. The
`preTokenBalance` and `postTokenBalance` fields from the transaction's metadata
must then be used to determine the effective balance change. These fields will
identify the token mint and account owner (main wallet address) of the affected
account.

Note that if a receiving account is created during the transaction, it will have no
`preTokenBalance` entry as there is no existing account state.  In this
case, the initial balance can be assumed to be zero.

### Withdrawing

The withdrawal address a user provides must be the that of their SOL wallet.

Before executing a withdrawal [transfer](#token-transfers),
the exchange should check the address as
[described above](#validating-user-supplied-account-addresses-for-withdrawals).
Additionally this address must be owned by the System Program and have no
account data. If the address has no SOL balance, user confirmation should be
obtained before proceeding with the withdrawal.  All other withdrawal addresses
must be rejected.

From the withdrawal address, the [Associated Token Account](https://spl.solana.com/associated-token-account)
(ATA) for the correct mint is derived and the transfer issued to that account via a
[TransferChecked](https://github.com/solana-labs/solana-program-library/blob/fc0d6a2db79bd6499f04b9be7ead0c400283845e/token/program/src/instruction.rs#L268)
instruction. Note that it is possible that the ATA address does not yet exist, at which point the
exchange should fund the account on behalf of the user. For SPL Token v2
accounts, funding the withdrawal account will require 0.00203928 SOL (2,039,280
lamports).

Template `spl-token transfer` command for a withdrawal:

```
$ spl-token transfer --fund-recipient <exchange token account> <withdrawal amount> <withdrawal address>
```

### Other Considerations

#### Freeze Authority

For regulatory compliance reasons, an SPL Token issuing entity may optionally
choose to hold "Freeze Authority" over all accounts created in association with
its mint. This allows them to [freeze](https://spl.solana.com/token#freezing-accounts)
the assets in a given account at will, rendering the account unusable until thawed.
If this feature is in use, the freeze authority's pubkey will be registered in
the SPL Token's mint account.

## Testing the Integration

Be sure to test your complete workflow on Solana devnet and testnet
[clusters](../clusters.md) before moving to production on mainnet-beta. Devnet
is the most open and flexible, and ideal for initial development, while testnet
offers more realistic cluster configuration. Both devnet and testnet support a faucet,
run `solana airdrop 1` to obtain some devnet or testnet SOL for development and testing.
