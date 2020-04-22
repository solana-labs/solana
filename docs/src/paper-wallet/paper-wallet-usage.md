# Paper Wallet Usage

Solana commands can be run without ever saving a keypair to disk on a machine.
If avoiding writing a private key to disk is a security concern of yours, you've
come to the right place.

{% hint style="warning" %}
Even using this secure input method, it's still possible that a private key gets
written to disk by unencrypted memory swaps. It is the user's responsibility to
protect against this scenario.
{% endhint %}

## Before You Begin

- [Install the Solana command-line tools](../cli/install-solana-cli-tools.md)

### Check your installation

Check that `solana-keygen` is installed correctly by running:

```bash
solana-keygen --version
```

## Creating a Paper Wallet

Using the `solana-keygen` tool, it is possible to generate new seed phrases as
well as derive a keypair from an existing seed phrase and (optional) passphrase.
The seed phrase and passphrase can be used together as a paper wallet. As long
as you keep your seed phrase and passphrase stored safely, you can use them to
access your account.

{% hint style="info" %}
For more information about how seed phrases work, review this
[Bitcoin Wiki page](https://en.bitcoin.it/wiki/Seed_phrase).
{% endhint %}

### Seed Phrase Generation

Generating a new keypair can be done using the `solana-keygen new` command. The
command will generate a random seed phrase, ask you to enter an optional
passphrase, and then will display the derived public key and the generated seed
phrase for your paper wallet.

After copying down your seed phrase, you can use the
[public key derivation](#public-key-derivation) instructions to verify that you
have not made any errors.

```bash
solana-keygen new --no-outfile
```

{% hint style="warning" %}
If the `--no-outfile` flag is **omitted**, the default behavior is to write the
keypair to `~/.config/solana/id.json`, resulting in a
[file system wallet](../file-system-wallet/README.md)
{% endhint %}

The output of this command will display a line like this:
```bash
pubkey: 9ZNTfG4NyQgxy2SWjSiQoUyBPEvXT2xo7fKc5hPYYJ7b
```

The value shown after `pubkey:` is your *wallet address*.

**Note:** In working with paper wallets and file system wallets, the terms "pubkey"
and "wallet address" are sometimes used interchangably.

{% hint style="info" %}
For added security, increase the seed phrase word count using the `--word-count`
argument
{% endhint %}

For full usage details run:

```bash
solana-keygen new --help
```

### Public Key Derivation

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

The `solana-keygen` tool uses the same BIP39 standard English word list as it
does to generate seed phrases. If your seed phrase was generated with another
tool that uses a different word list, you can still use `solana-keygen`, but
will need to pass the `--skip-seed-phrase-validation` argument and forego this
validation.

```bash
solana-keygen pubkey ASK --skip-seed-phrase-validation
```

After entering your seed phrase with `solana-keygen pubkey ASK` the console
will display a string of base-58 character.  This is the *wallet address*
associated with your seed phrase.

{% hint style="info" %}
Copy the derived address to a USB stick for easy usage on networked computers
{% endhint %}

{% hint style="info" %}
A common next step is to [check the balance](#checking-account-balance) of the
account associated with a public key
{% endhint %}

For full usage details run:

```bash
solana-keygen pubkey --help
```

## Verifying the Keypair

To verify you control the private key of a paper wallet address, use
`solana-keygen verify`:

```bash
solana-keygen verify <PUBKEY> ASK
```

where `<PUBKEY>` is replaced with the wallet address and they keyword `ASK` tells the
command to prompt you for the keypair's seed phrase. Note that for security
reasons, your seed phrase will not be displayed as you type. After entering your
seed phrase, the command will output "Success" if the given public key matches the
keypair generated from your seed phrase, and "Failed" otherwise.

## Checking Account Balance

All that is needed to check an account balance is the public key of an account.
To retrieve public keys securely from a paper wallet, follow the
[Public Key Derivation](#public-key-derivation) instructions on an
[air gapped computer](https://en.wikipedia.org/wiki/Air_gap_\(networking\)).
Public keys can then be typed manually or transferred via a USB stick to a
networked machine.

Next, configure the `solana` CLI tool to
[connect to a particular cluster](../cli/choose-a-cluster.md):

```bash
solana config set --url <CLUSTER URL> # (i.e. https://api.mainnet-beta.solana.com)
```

Finally, to check the balance, run the following command:

```bash
solana balance <PUBKEY>
```

## Creating Multiple Paper Wallet Addresses
You can create as many wallet addresses as you like.  Simply re-run the
steps in [Seed Phrase Generation](#seed-phrase-generation) or
[Public Key Derivation](#public-key-derivation) to create a new address.
Multiple wallet addresses can be useful if you want to transfer tokens between
your own accounts for different purposes.

## Support

Check out our [Wallet Support Page](../wallet-guide/support.md) for ways to get help.
