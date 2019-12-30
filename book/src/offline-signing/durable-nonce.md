# Durable Transaction Nonces

Durable transaction nonces are a mechanism for getting around the typical
short lifetime of a transaction's [`recent_blockhash`](../transaction.md#recent-blockhash).
They are implemented as a Solana Program, the mechanics of which can be read
about in the [proposal](../implemented-proposals/durable-tx-nonces.md).

## Known Issues

### Fee Theft Opportunity

The durable nonce implementation contains a vulernability which allows for fees
to be stolen by a transaction using the feature under certain conditions. If the
transaction fails with an instruction errror, the runtime rolls back the step
that advanced the stored nonce, allowing it to be replayed and fees charged.
This can be repeated until the stored nonce is successfully advanced.

- Mitigation

To minimize loss of funds, use a low-balance account to pay fees on a durable
nonce transaction.

If a transaction using the durable nonce feature fails with an instruction error,
immediately submit a new transaction that advances the nonce and will certainly
succeed. The simplest way to do this is with a single-instruction
`NonceInstruction::Nonce` transaction, which can be sent using the CLI
[`new-nonce`](#advancing-the-stored-nonce-value) command.

- Issue Tracking

This issue is being actively addressed, progress can be followed on
[Github](https://github.com/solana-labs/solana/issues/7443).

## Usage Examples

Full usage details for durable nonce CLI commands can be found in the
[CLI reference](../api-reference/cli.md).

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
value.  Durable nonce accounts must be [rent-exempt](../implemented-proposals/rent.md#two-tiered-rent-regime),
so need to carry the minimum balance to acheive this.

A nonce account is created by first generating a new keypair, then create the account on chain

- Command

```bash
solana-keygen new -o nonce-keypair.json
solana create-nonce-account nonce-keypair.json 1 SOL
```

- Output

```text
2SymGjGV4ksPdpbaqWFiDoBz8okvtiik4KE9cnMQgRHrRLySSdZ6jrEcpPifW4xUpp4z66XM9d9wM48sA7peG2XL
```

{% hint style="info" %}
To keep the keypair entirely offline, use the [Paper Wallet](../paper-wallet/README.md)
keypair generation [instructions](../paper-wallet/usage.md#seed-phrase-generation.md)
instead
{% endhint %}

{% hint style="info" %}
[Full usage documentation](../api-reference/cli.md#solana-create-nonce-account)
{% endhint %}

### Querying the Stored Nonce Value

Creating a durable nonce transaction requires passing the stored nonce value as
the value to the `--blockhash` argument upon signing and submission. Obtain the
presently stored nonce value with

- Command

```bash
solana get-nonce nonce-keypair.json 
```

- Output

```text
8GRipryfxcsxN8mAGjy8zbFo9ezaUsh47TsPzmZbuytU
```

{% hint style="info" %}
[Full usage documentation](../api-reference/cli.md#solana-get-nonce)
{% endhint %}

### Advancing the Stored Nonce Value

While not typically needed outside a more useful transaction, the stored nonce
value can be advanced by

- Command

```bash
solana new-nonce nonce-keypair.json
```

- Output

```text
44jYe1yPKrjuYDmoFTdgPjg8LFpYyh1PFKJqm5SC1PiSyAL8iw1bhadcAX1SL7KDmREEkmHpYvreKoNv6fZgfvUK
```

{% hint style="info" %}
[Full usage documentation](../api-reference/cli.md#solana-new-nonce)
{% endhint %}

### Display Nonce Account

Inspect a nonce account in a more human friendly format with

- Command

```bash
solana show-nonce-account nonce-keypair.json 
```

- Output

```text
balance: 0.5 SOL
minimum balance required: 0.00136416 SOL
nonce: DZar6t2EaCFQTbUP4DHKwZ1wT8gCPW2aRfkVWhydkBvS
```

{% hint style="info" %}
[Full usage documentation](../api-reference/cli.md#solana-show-nonce-account)
{% endhint %}

### Withdraw Funds from a Nonce Account

Withdraw funds from a nonce account with

- Command

```bash
solana withdraw-from-nonce-account nonce-keypair.json ~/.config/solana/id.json 0.5 SOL
```

- Output

```text
3foNy1SBqwXSsfSfTdmYKDuhnVheRnKXpoPySiUDBVeDEs6iMVokgqm7AqfTjbk7QBE8mqomvMUMNQhtdMvFLide
```

{% hint style="info" %}
Close a nonce account by withdrawing the full balance
{% endhint %}

{% hint style="info" %}
[Full usage documentation](../api-reference/cli.md#solana-withdraw-from-nonce-account)
{% endhint %}

### Assign a New Authority to a Nonce Account

Reassign the authority of a nonce account after creation with

- Command

```bash
solana authorize-nonce-account nonce-keypair.json nonce-authority.json 
```

- Output

```text
3F9cg4zN9wHxLGx4c3cUKmqpej4oa67QbALmChsJbfxTgTffRiL3iUehVhR9wQmWgPua66jPuAYeL1K2pYYjbNoT
```

{% hint style="info" %}
[Full usage documentation](../api-reference/cli.md#solana-authorize-nonce-account)
{% endhint %}

