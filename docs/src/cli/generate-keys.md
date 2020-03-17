# Generate Keys

## Generate an FS Wallet Keypair

Use Solana's command-line tool `solana-keygen` to generate keypair files. For
example, run the following from a command-line shell:

```bash
mkdir ~/my-solana-wallet
solana-keygen new -o ~/my-solana-wallet/my-keypair.json
```

If you view the file, you will a long list of numbers, such as:

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


## Generate a Paper Wallet Keypair

See [Paper Wallets](paper_wallet.md).

## Generate a Hardware Wallet Keypair

See [Hardware Wallets](../remote-wallet).
