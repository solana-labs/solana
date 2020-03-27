# Generate a Keypair and its Public Key

In this section, we will generate a keypair, query it for its public key,
and verify you control its private key. Before you begin, you will need
to:

* [Install the Solana Tool Suite](install-solana-cli-tools.md)
* [Choose a wallet](cli-wallets.md)

## Generate an FS Wallet Keypair

Use Solana's command-line tool `solana-keygen` to generate keypair files. For
example, run the following from a command-line shell:

```bash
mkdir ~/my-solana-wallet
solana-keygen new -o ~/my-solana-wallet/my-keypair.json
```

If you view the file, you will see a long list of numbers, such as:

```text
[42,200,155,187,52,228,32,9,179,129,192,196,149,41,177,47,87,228,5,19,70,82,170,6,142,114,68,85,124,34,165,216,110,186,177,254,198,143,235,59,173,59,17,250,142,32,66,162,130,62,53,252,48,33,148,38,149,17,81,154,95,178,163,164]
```

This file contains your **unencrypted** keypair. In fact, even if you specify
a password, that password applies to the recovery seed phrase, not the file. Do
not share this file with others. Anyone with access to this file will have access
to all tokens sent to its public key. Instead, you should share only its public
key. To display its public key, run:

```bash
solana-keygen pubkey ~/my-solana-wallet/my-keypair.json
```

It will output a string of characters, such as:

```text
ErRr1caKzK8L8nn4xmEWtimYRiTCAZXjBtVphuZ5vMKy
```

This is the public key corresponding to the keypair in `~/my-solana-wallet/my-keypair.json`.
To verify you hold the private key for a given public key, use `solana-keygen verify`:

```bash
solana-keygen verify <PUBKEY> ~/my-solana-wallet/my-keypair.json
```

where `<PUBKEY>` is the public key output from the previous command.
The command will output "Success" if the given public key matches the
the one in your keypair file, and "Failed" otherwise.

## Generate a Paper Wallet Seed Phrase

See [Creating a Paper Wallet](../paper-wallet/paper-wallet-usage.md#creating-a-paper-wallet).

To verify you control the private key of that public key, use `solana-keygen verify`:

```bash
solana-keygen verify <PUBKEY> ASK
```

where `<PUBKEY>` is the keypair's public key and they keyword `ASK` tells the
command to prompt you for the keypair's seed phrase. Note that for security
reasons, your seed phrase will not be displayed as you type. After entering your
seed phrase, the command will output "Success" if the given public key matches the
keypair generated from your seed phrase, and "Failed" otherwise.

## Generate a Hardware Wallet Keypair

Keypairs are automatically derived when you query a hardware wallet with a
[keypair URL](../remote-wallet#specify-a-hardware-wallet-key).

Once you have your keypair URL, use `solana-keygen pubkey` to query the hardware
wallet for the keypair's public key:

```bash
solana-keygen pubkey <KEYPAIR>
```

where `<KEYPAIR>` is the keypair URL.

To verify you control the private key of that public key, use `solana-keygen verify`:

```bash
solana-keygen verify <PUBKEY> <KEYPAIR>
```

The command will output "Success" if the given public key matches the
the one at your keypair URL, and "Failed" otherwise.
