# Offline Transaction Signing

Some security models require keeping signing keys, and thus the signing
process, separated from transaction creation and network broadcast. Examples
include:
  * Collecting signatures from geographically disparate signers in a
[multi-signature scheme](../api-reference/cli.md#multiple-witnesses)
  * Signing transactions using an [airgapped](https://en.wikipedia.org/wiki/Air_gap_(networking))
signing device

This document describes using Solana's CLI to separately sign and submit a
transaction.

## Commands Supporting Offline Signing

At present, the following commands support offline signing:
  * [`delegate-stake`](../api-reference/cli.md#solana-delegate-stake)
  * [`deactivate-stake`](../api-reference/cli.md#solana-deactivate-stake)
  * [`pay`](../api-reference/cli.md#solana-pay)

## Signing Transactions Offline

To sign a transaction offline, pass the following arguments on the command line
1) `--sign-only`, prevents the client from submitting the signed transaction
to the network. Instead, the pubkey/signature pairs are printed to stdout.
2) `--blockhash BASE58_HASH`, allows the caller to specify the value used to
fill the transaction's `recent_blockhash` field. This serves a number of
purposes, namely:
    * Eliminates the need to connect to the network and query a recent blockhash
via RPC
    * Enables the signers to coordinate the blockhash in a multiple-signature
scheme

### Example: Offline Signing a Payment

Command

```bash
solana@offline$ solana pay --sign-only --blockhash 5Tx8F3jgSHx21CbtjwmdaKPLM5tWmreWAnPrbqHomSJF \
    recipient-keypair.json 1
```

Output

```text

Blockhash: 5Tx8F3jgSHx21CbtjwmdaKPLM5tWmreWAnPrbqHomSJF
Signers (Pubkey=Signature):
  FhtzLVsmcV7S5XqGD79ErgoseCLhZYmEZnz9kQg1Rp7j=4vC38p4bz7XyiXrk6HtaooUqwxTWKocf45cstASGtmrD398biNJnmTcUCVEojE7wVQvgdYbjHJqRFZPpzfCQpmUN

{"blockhash":"5Tx8F3jgSHx21CbtjwmdaKPLM5tWmreWAnPrbqHomSJF","signers":["FhtzLVsmcV7S5XqGD79ErgoseCLhZYmEZnz9kQg1Rp7j=4vC38p4bz7XyiXrk6HtaooUqwxTWKocf45cstASGtmrD398biNJnmTcUCVEojE7wVQvgdYbjHJqRFZPpzfCQpmUN"]}'
```

## Submitting Offline Signed Transactions to the Network

To submit a transaction that has been signed offline to the network, pass the
following arguments on the command line
1) `--blockhash BASE58_HASH`, must be the same blockhash as was used to sign
2) `--signer BASE58_PUBKEY=BASE58_SIGNATURE`, one for each offline signer. This
includes the pubkey/signature pairs directly in the transaction rather than
signing it with any local keypair(s)

### Example: Submitting an Offline Signed Payment

Command

```bash
solana@online$ solana pay --blockhash 5Tx8F3jgSHx21CbtjwmdaKPLM5tWmreWAnPrbqHomSJF \
    --signer FhtzLVsmcV7S5XqGD79ErgoseCLhZYmEZnz9kQg1Rp7j=4vC38p4bz7XyiXrk6HtaooUqwxTWKocf45cstASGtmrD398biNJnmTcUCVEojE7wVQvgdYbjHJqRFZPpzfCQpmUN
    recipient-keypair.json 1
```

Output

```text
4vC38p4bz7XyiXrk6HtaooUqwxTWKocf45cstASGtmrD398biNJnmTcUCVEojE7wVQvgdYbjHJqRFZPpzfCQpmUN
```

## Buying More Time to Sign

Typically a Solana transaction must be signed and accepted by the network within
a number of slots from the blockhash in its `recent_blockhash` field (~2min at
the time of this writing). If your signing procedure takes longer than this, a
[Durable Transaction Nonce](durable-nonce.md) can give you the extra time you
need.
