# Offline Transaction Signing

Some security models require keeping signing keys, and thus the signing
process, separated from transaction creation and network broadcast. Examples
include:
  * Collecting signatures from geographically disparate signers in a
multi-signature scheme
  * Signing transactions using an [airgapped](https://en.wikipedia.org/wiki/Air_gap_(networking))
signing device

To facilitate these operations, it is necessary to allow transaction
construction and signing to occur independently.

This document describes using Solana's CLI tools achieve these goals.

## Commands Supporting Offline Signing

At present, the following commands support offline signing:
  * [`deactivate-stake`](../api-reference/cli.md#solana-deactivate-stake)
  * [`delegate-stake`](../api-reference/cli.md#solana-delegate-stake)
  * [`pay`](../api-reference/cli.md#solana-pay)

## Signing Transactions Offline

Signing a transaction offline hinges upon passing two arguments on the command
line. The first, `--sign-only`, prevents the client from submitting the signed
transaction to the network. Instead, the pubkey/signature pairs are printed to
stdout. The second argument, `--blockhash BASE58_HASH`, allows the caller to
specify the value used to fill the transaction's `recent_blockhash` field. This
serves a number of purposes, namely:
  * Eliminates the need to connect to the network and query a recent blockhash
via RPC
  * Enables the signers to coordinate the blockhash in a multiple-signature
scheme

### Example payment (cont'd below)

Command

```bash
solana@offline$ solana --no-header pay --sign-only --blockhash \
5Tx8F3jgSHx21CbtjwmdaKPLM5tWmreWAnPrbqHomSJF recipient-keypair.json 1 SOL
```

Output

```text

Blockhash: 5Tx8F3jgSHx21CbtjwmdaKPLM5tWmreWAnPrbqHomSJF
Signers (Pubkey=Signature):
  FhtzLVsmcV7S5XqGD79ErgoseCLhZYmEZnz9kQg1Rp7j=4vC38p4bz7XyiXrk6HtaooUqwxTWKocf45cstASGtmrD398biNJnmTcUCVEojE7wVQvgdYbjHJqRFZPpzfCQpmUN

{"blockhash":"5Tx8F3jgSHx21CbtjwmdaKPLM5tWmreWAnPrbqHomSJF","signers":["FhtzLVsmcV7S5XqGD79ErgoseCLhZYmEZnz9kQg1Rp7j=4vC38p4bz7XyiXrk6HtaooUqwxTWKocf45cstASGtmrD398biNJnmTcUCVEojE7wVQvgdYbjHJqRFZPpzfCQpmUN"]}'
```

## Submitting Offline Signed Transactions to the Network

Submitting a transaction which has been signed offline to the network is done
by passing the pubkey/signature pair for each signer on the command line. This
is achieved by the `--signer BASE58_PUBKEY=BASE58_SIGNATURE` argument, one per
signer. When this argument is passed, the pubkey/signature pairs are embedded
instead of signing with any local keypair.

### Example payment (cont'd from above)

Command

```bash
solana@online$ solana --no-header pay --blockhash 5Tx8F3jgSHx21CbtjwmdaKPLM5tWmreWAnPrbqHomSJF \
recipient-keypair.json 1 SOL --signer \
FhtzLVsmcV7S5XqGD79ErgoseCLhZYmEZnz9kQg1Rp7j=4vC38p4bz7XyiXrk6HtaooUqwxTWKocf45cstASGtmrD398biNJnmTcUCVEojE7wVQvgdYbjHJqRFZPpzfCQpmUN
```

Output

```text
4vC38p4bz7XyiXrk6HtaooUqwxTWKocf45cstASGtmrD398biNJnmTcUCVEojE7wVQvgdYbjHJqRFZPpzfCQpmUN
```
