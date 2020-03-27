# Durable Transaction Nonces

Durable transaction nonces are a mechanism for getting around the typical
short lifetime of a transaction's [`recent_blockhash`](../transaction.md#recent-blockhash).
They are implemented as a Solana Program, the mechanics of which can be read
about in the [proposal](../implemented-proposals/durable-tx-nonces.md).

## Usage Examples

Full usage details for durable nonce CLI commands can be found in the
[CLI reference](../api-reference/cli.md).

### Nonce Authority

Authority over a nonce account can optionally be assigned to another account. In
doing so the new authority inherits full control over the nonce account from the
previous authority, including the account creator. This feature enables the
creation of more complex account ownership arrangements and derived account
addresses not associated with a keypair. The `--nonce-authority <AUTHORITY_KEYPAIR>`
argument is used to specify this account and is supported by the following
commands
* `create-nonce-account`
* `new-nonce`
* `withdraw-from-nonce-account`
* `authorize-nonce-account`

### Nonce Account Creation

The durable transaction nonce feature uses an account to store the next nonce
value.  Durable nonce accounts must be [rent-exempt](../implemented-proposals/rent.md#two-tiered-rent-regime),
so need to carry the minimum balance to achieve this.

A nonce account is created by first generating a new keypair, then create the account on chain

- Command

```bash
solana-keygen new -o nonce-keypair.json
solana create-nonce-account nonce-keypair.json 1
```

- Output

```text
2SymGjGV4ksPdpbaqWFiDoBz8okvtiik4KE9cnMQgRHrRLySSdZ6jrEcpPifW4xUpp4z66XM9d9wM48sA7peG2XL
```

{% hint style="info" %}
To keep the keypair entirely offline, use the [Paper Wallet](../paper-wallet/README.md)
keypair generation [instructions](../paper-wallet/paper-wallet-usage.md#seed-phrase-generation.md)
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
solana nonce nonce-keypair.json
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
solana nonce-account nonce-keypair.json
```

- Output

```text
balance: 0.5 SOL
minimum balance required: 0.00136416 SOL
nonce: DZar6t2EaCFQTbUP4DHKwZ1wT8gCPW2aRfkVWhydkBvS
```

{% hint style="info" %}
[Full usage documentation](../api-reference/cli.md#solana-nonce-account)
{% endhint %}

### Withdraw Funds from a Nonce Account

Withdraw funds from a nonce account with

- Command

```bash
solana withdraw-from-nonce-account nonce-keypair.json ~/.config/solana/id.json 0.5
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

## Other Commands Supporting Durable Nonces

To make use of durable nonces with other CLI subcommands, two arguments must be
supported.
* `--nonce`, specifies the account storing the nonce value
* `--nonce-authority`, specifies an optional [nonce authority](#nonce-authority)

The following subcommands have received this treatment so far
* [`pay`](../api-reference/cli.md#solana-pay)
* [`delegate-stake`](../api-reference/cli.md#solana-delegate-stake)
* [`deactivate-stake`](../api-reference/cli.md#solana-deactivate-stake)

### Example Pay Using Durable Nonce

Here we demonstrate Alice paying Bob 1 SOL using a durable nonce. The procedure
is the same for all subcommands supporting durable nonces

#### - Create accounts

First we need some accounts for Alice, Alice's nonce and Bob

```bash
$ solana-keygen new -o alice.json
$ solana-keygen new -o nonce.json
$ solana-keygen new -o bob.json
```

#### - Fund Alice's account

Alice will need some funds to create a nonce account and send to Bob. Airdrop
her some SOL

```bash
$ solana airdrop -k alice.json 10
10 SOL
```

#### - Create Alice's nonce account

Now Alice needs a nonce account. Create one

{% hint style="info" %}
Here, no separate [nonce authority](#nonce-authority) is employed, so `alice.json`
has full authority over the nonce account
{% endhint %}

```bash
$ solana create-nonce-account -k alice.json nonce.json 1
3KPZr96BTsL3hqera9up82KAU462Gz31xjqJ6eHUAjF935Yf8i1kmfEbo6SVbNaACKE5z6gySrNjVRvmS8DcPuwV
```

#### - A failed first attempt to pay Bob

Alice attempts to pay Bob, but takes too long to sign. The specified blockhash
expires and the transaction fails

```bash
$ solana pay -k alice.json --blockhash expiredDTaxfagttWjQweib42b6ZHADSx94Tw8gHx3W7 bob.json 1
[2020-01-02T18:48:28.462911000Z ERROR solana_cli::cli] Io(Custom { kind: Other, error: "Transaction \"33gQQaoPc9jWePMvDAeyJpcnSPiGUAdtVg8zREWv4GiKjkcGNufgpcbFyRKRrA25NkgjZySEeKue5rawyeH5TzsV\" failed: None" })
Error: Io(Custom { kind: Other, error: "Transaction \"33gQQaoPc9jWePMvDAeyJpcnSPiGUAdtVg8zREWv4GiKjkcGNufgpcbFyRKRrA25NkgjZySEeKue5rawyeH5TzsV\" failed: None" })
```

#### - Nonce to the rescue!

Alice retries the transaction, this time specifying her nonce account and the
blockhash stored there

{% hint style="info" %}
Remember, `alice.json` is the [nonce authority](#nonce-authority) in this example
{% endhint %}

```bash
$ solana nonce-account nonce.json
balance: 1 SOL
minimum balance required: 0.00136416 SOL
nonce: F7vmkY3DTaxfagttWjQweib42b6ZHADSx94Tw8gHx3W7
```
```bash
$ solana pay -k alice.json --blockhash F7vmkY3DTaxfagttWjQweib42b6ZHADSx94Tw8gHx3W7 --nonce nonce.json bob.json 1
HR1368UKHVZyenmH7yVz5sBAijV6XAPeWbEiXEGVYQorRMcoijeNAbzZqEZiH8cDB8tk65ckqeegFjK8dHwNFgQ
```

#### - Success!

The transaction succeeds!  Bob receives 1 SOL from Alice and Alice's stored
nonce advances to a new value

```bash
$ solana balance -k bob.json
1 SOL
```
```bash
$ solana nonce-account nonce.json
balance: 1 SOL
minimum balance required: 0.00136416 SOL
nonce: 6bjroqDcZgTv6Vavhqf81oBHTv3aMnX19UTB51YhAZnN
```
