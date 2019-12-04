# Creating a Paper Wallet

Using the `solana-keygen` tool, it is possible to generate new seed phrases as
well as derive a keypair from an existing seed phrase and (optional) passphrase.
The seed phrase and passphrase can be used together as a paper wallet. As long
as you keep your seed phrase and passphrase stored safely, you can use them to
access your account.

{% hint style="info" %}
For more information about how seed phrases work, review this
[Bitcoin Wiki page](https://en.bitcoin.it/wiki/Seed_phrase).
{% endhint %}

## Seed Phrase Generation

Generating a new keypair can be done using the `solana-keygen new` command. The
command will generate a random seed phrase, ask you to enter an optional
passphrase, and then will display the derived public key and the generated seed
phrase for your paper wallet.

```bash
solana-keygen new --no-outfile
```

{% hint style="warning" %}
If `--no-outfile` is **not** specified, the default behavior is to write the
keypair to `~/.config/solana/id.json`
{% endhint %}

For full usage details run:

```bash
solana-keygen new --help
```

## Public Key Derivation

Public keys can be derived from a seed phrase and a passphrase if you choose to
use one. This is useful for using an offline-generated seed phrase to
derive a valid public key. The `solana-keygen pubkey` command will walk you
through entering your seed phrase and a passphrase if you chose to use one.

```bash
solana-keygen pubkey ASK
```

{% hint style="info" %}
Note that you could potentially use different passphrases for the same seed
phrase. Each unique passphrase will yield a different keypair.
{% endhint %}

The `solana-keygen` tool assumes the use of the BIP39 standard English word
list. If you chose to deviate from the word list or used a different language
for your seed phrase, you can still derive a valid public key but will need to
explicitly skip seed phrase validation.

```bash
solana-keygen pubkey ASK --skip-seed-phrase-validation
```

For full usage details run:

```bash
solana-keygen pubkey --help
```
