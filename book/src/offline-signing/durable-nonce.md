# Durable Transaction Nonces

Durable transaction nonces are a mechanism for getting around the typical
short lifetime of a transaction's [`recent_blockhash`](../transaction.md#recent-blockhash).
They are implemented as a Solana Program, the mechanics of which can be read
about in the [proposal](../implemented-proposals/durable-tx-nonces.md).

## Known Issues

### Fee Theft Opportunity

The durable nonce implementation contains a vulernability which allows for fees
to be stolen by a transaction using the feature under certain conditions. To
achieve this, the transaction must have failed with an instruction error, which
rolls back the step that advanced the stored nonce, allowing the transaction to
be replayed and fees charged. This can be repeated until the stored nonce is
successfully advanced.

#### - Mitigation

To minimize loss of funds, a low-balance account should always be used to pay
fees on a durable nonce transaction.

If a transaction using the durable nonce feature fails with an instruction error,
a new transaction, which only advances the nonce must be submitted to the network
immediately.

#### - Issue Tracking

This issue is being actively addressed, progress can be followed on
[Github](https://github.com/solana-labs/solana/issues/7443).

## Usage

The following are instructions for using the durable transaction nonce feature
with Solana's CLI tools.

Additionally, authority over a nonce account can be assigned to another entity.
This enables the creation of more complex account ownership arrangements and
derived account addresses not associated with a keypair. The
`--nonce-authority <AUTHORITY_KEYPAIR>` argument is used to specify this
authority and is supported by the following commands
* `create-nonce-account`
* `new-nonce`
* `withdraw-from-nonce-account`
* `authorize-nonce-account`

### Nonce Account Creation

The durable transaction nonce feature uses an account to store the next nonce
value.  Durable nonce accounts must be [rent-exempt](proposals/rent.md#two-tiered-rent-regime),
so need to carry the minimum balance to acheive this.

A nonce account is created by first generating a new keypair

```bash
solana-keygen new -o <NONCE_KEYPAIR>
```

{% hint style="info" %}
To keep the keypair entirely offline, use the [Paper Wallet](../paper-wallet/README.md)
keypair generation [instructions](../paper-wallet/usage.md#seed-phrase-generation.md)
instead
{% endhint %}

Then create the account on chain

```bash
solana create-nonce-account <NONCE_KEYPAIR> <AMOUNT>
```

#### Example

Command

```bash
solana create-nonce-account nonce-keypair.json 1 SOL
```

Output

```text
2SymGjGV4ksPdpbaqWFiDoBz8okvtiik4KE9cnMQgRHrRLySSdZ6jrEcpPifW4xUpp4z66XM9d9wM48sA7peG2XL
```

### Querying the Stored Nonce Value

Creating a durable nonce transaction requires passing the stored nonce value as
the value to the `--blockhash` argument upon signing and submission. Obtain the
presently stored nonce value with

```bash
solana get-nonce <NONCE_ACCOUNT>
```

#### Example

Command

```bash
solana get-nonce nonce-keypair.json 
```

Output

```text
8GRipryfxcsxN8mAGjy8zbFo9ezaUsh47TsPzmZbuytU
```

### Advancing the Stored Nonce Value

While not typically needed outside a more useful transaction, the stored nonce
value can be advanced by

```bash
solana new-nonce <NONCE_ACCOUNT>
```

#### Example

Command

```bash
solana new-nonce nonce-keypair.json
```

Output

```text
44jYe1yPKrjuYDmoFTdgPjg8LFpYyh1PFKJqm5SC1PiSyAL8iw1bhadcAX1SL7KDmREEkmHpYvreKoNv6fZgfvUK
```

### Display Nonce Account

Inspect a nonce account in a more human friendly format with

```bash
solana show-nonce-account <NONCE_ACCOUNT>
```

#### Example

Command

```bash
solana show-nonce-account nonce-keypair.json 
```

Output

```text
balance: 0.5 SOL
minimum balance required: 0.00136416 SOL
nonce: DZar6t2EaCFQTbUP4DHKwZ1wT8gCPW2aRfkVWhydkBvS
```

### Withdraw Funds from a Nonce Account

Withdraw funds from a nonce account with

```bash
solana withdraw-from-nonce-account <NONCE_ACCOUNT> <TO_ACCOUNT> <AMOUNT>
```

{% hint style="info" %}
Close a nonce account by withdrawing the full balance
{% endhint %}

#### Example

Command

```bash
solana withdraw-from-nonce-account nonce-keypair.json ~/.config/solana/id.json 0.5 SOL
```

Output

```text
3foNy1SBqwXSsfSfTdmYKDuhnVheRnKXpoPySiUDBVeDEs6iMVokgqm7AqfTjbk7QBE8mqomvMUMNQhtdMvFLide
```

### Assign a New Authority to a Nonce Account

```bash
solana authorize-nonce-account <NONCE_ACCOUNT> <NEW_AUTHORITY_KEYPAIR>
```

#### Example

Command

```bash
solana authorize-nonce-account nonce-keypair.json nonce-authority.json 
```

Output

```text
3F9cg4zN9wHxLGx4c3cUKmqpej4oa67QbALmChsJbfxTgTffRiL3iUehVhR9wQmWgPua66jPuAYeL1K2pYYjbNoT
```
