# Keypair Generation and Recovery

Using the `solana-keygen` tool, it is possible to generate new seed phrases for key derivation as well as recover keypairs directly from a seed phrase and optional passphrase. The seed phrase and passphrase together can be used as a paper wallet.

{% hint style="info" %}
We do not offer advice on how to securely handle your paper wallet. Please research the security concerns carefully.
{% endhint %}

## Keypair Generation

Keypair creation can be done using the `solana-keygen new` command. The command will walk you through choosing an optional passphrase and will display the generated seed phrase for your paper wallet.

```bash
solana-keygen new --no-outfile
```

{% hint style="warning" %}
If `--no-outfile` is not specified, the default behavior is to write the keypair to `~/.config/solana/id.json`
{% endhint %}

Full usage details:

```bash
Generate new keypair file from a passphrase and random seed phrase

USAGE:
    solana-keygen new [FLAGS] [OPTIONS]

FLAGS:
    -f, --force            Overwrite the output file if it exists
    -h, --help             Prints help information
        --no-outfile       Only print a seed phrase and pubkey. Do not output a keypair file
        --no-passphrase    Do not prompt for a passphrase
    -s, --silent           Do not display seed phrase. Useful when piping output to other programs that prompt for user
                           input, like gpg

OPTIONS:
    -o, --outfile <PATH>    Path to generated file
```

## Keypair Recovery

Keypair recovery can be done using the `solana-keygen recover` command. This is useful for using your paper wallet to create a keypair file on a machine for easy access. The `solana-keygen recover` command will walk you through entering your seed phrase and a passphrase if you chose to use one.

```bash
solana-keygen recover
```

{% hint style="info" %}
If an `--outfile` is not specified, the default behavior is to write the keypair to `~/.config/solana/id.json`
{% endhint %}

Full usage details:

```bash
Recover keypair from seed phrase and passphrase

USAGE:
    solana-keygen recover [FLAGS] [OPTIONS]

FLAGS:
    -f, --force                          Overwrite the output file if it exists
    -h, --help                           Prints help information
        --skip-seed-phrase-validation    Skip validation of seed phrases. Use this if your phrase does not use the BIP39
                                         official English word list

OPTIONS:
    -o, --outfile <PATH>    Path to generated file
```
