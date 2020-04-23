# Send and Receive Tokens

This page decribes how to receive and send SOL tokens using the command line
tools with a command line wallet such as a [paper wallet](../paper-wallet/README.md),
a [file system wallet](../file-system-wallet/README.md), or a
[hardware wallet](../hardware-wallets/README.md).  Before you begin, make sure
you have created a wallet and have access to its address (pubkey) and the
signing keypair.  Check out our
[conventions for entering keypairs for different wallet types](../cli/conventions.md#keypair-conventions).

## Receive Tokens

To receive tokens, you will need an address for others to send tokens to. In
Solana, an address is the public key of a keypair. There are a variety
of techniques for generating keypairs. The method you choose will depend on how
you choose to store keypairs.  Keypairs are stored in wallets. Before receiving
tokens, you will need to [choose a wallet](../wallet/cli-wallets.md) and
[generate keys](generate-keys.md). Once completed, you should have a public key
for each keypair you generated. The public key is a long string of base58
characters. Its length varies from 32 to 44 characters.

### Using Solana CLI

Before running any Solana CLI commands, let's go over some conventions that
you will see across all commands. First, the Solana CLI is actually a collection
of different commands for each action you might want to take. You can view the list
of all possible commands by running:

```bash
solana --help
```

To zoom in on how to use a particular command, run:

```bash
solana <COMMAND> --help
```

where you replace the text `<COMMAND>` with the name of the command you want
to learn more about.

The command's usage message will typically contain words such as `<AMOUNT>`,
`<ACCOUNT_ADDRESS>` or `<KEYPAIR>`. Each word is a placeholder for the *type* of
text you can execute the command with. For example, you can replace `<AMOUNT>`
with a number such as `42` or `100.42`. You can replace `<ACCOUNT_ADDRESS>` with
the base58 encoding of your public key. For `<KEYPAIR>`, it depends on what type
of wallet you chose. If you chose an fs wallet, that path might be
`~/my-solana-wallet/my-keypair.json`.  If you chose a paper wallet, use the
keyword `ASK`, and the Solana CLI will prompt you for your seed phrase. If
you chose a hardware wallet, use your keypair URL, such as `usb://ledger?key=0`.

### Test-drive your Public Keys

Before sharing your public key with others, you may want to first ensure the
key is valid and that you indeed hold the corresponding private key.

Try and *airdrop* yourself some play tokens on the developer testnet, called
Devnet:

```bash
solana airdrop 10 <RECIPIENT_ACCOUNT_ADDRESS> --url http://devnet.solana.com
```

where you replace the text `<RECIPIENT_ACCOUNT_ADDRESS>` with your base58-encoded
public key.

Confirm the airdrop was successful by checking the account's balance.
It should output `10 SOL`:

```bash
solana balance <ACCOUNT_ADDRESS> --url http://devnet.solana.com
```

Next, prove that you own those tokens by transferring them. The Solana cluster
will only accept the transfer if you sign the transaction with the private
key corresponding to the sender's public key in the transaction.

First, we will need a public key to receive our tokens. Create a second
keypair and record its pubkey:

```bash
solana-keygen new --no-passphrase --no-outfile
```

The output will contain the public key after the text `pubkey:`. Copy the
public key. We will use it in the next step.

```text
============================================================================
pubkey: GKvqsuNcnwWqPzzuhLmGi4rzzh55FhJtGizkhHaEJqiV
============================================================================
```

You can also create a second (or more) wallet of any type:
[paper](../paper-wallet/paper-wallet-usage.md#creating-multiple-paper-wallet-addresses),
[file system](../file-system-wallet/README.md#creating-multiple-file-system-wallet-addresses),
or [hardware](../hardware-wallets/README.md#multiple-addresses-on-a-single-hardware-wallet).

#### Transfer tokens from your first wallet to the second address

Next, prove that you own the airdropped tokens by transferring them.
The Solana cluster will only accept the transfer if you sign the transaction
with the private keypair corresponding to the sender's public key in the
transaction.

```bash
solana transfer --from <KEYPAIR> <RECIPIENT_ACCOUNT_ADDRESS> 5--url http://devnet.solana.com --fee-payer <KEYPAIR>
```

where you replace `<KEYPAIR>` with the path to a keypair in your wallet,
and replace `<RECIPIENT_ACCOUNT_ADDRESS>` with the output of `solana-keygen new` above.

Confirm the updated balances with `solana balance`:

```bash
solana balance <ACCOUNT_ADDRESS> --url http://devnet.solana.com
```

where `<ACCOUNT_ADDRESS>` is either the public key from your keypair or the
recipient's public key.

#### Full example of test transfer
```bash
$ solana-keygen new --outfile my_solana_wallet.json   # Creating my first wallet, a file system wallet
Generating a new keypair
For added security, enter a passphrase (empty for no passphrase):
Wrote new keypair to my_solana_wallet.json
==========================================================================
pubkey: DYw8jCTfwHNRJhhmFcbXvVDTqWMEVFBX6ZKUmG5CNSKK                          # Here is the address of the first wallet
==========================================================================
Save this seed phrase to recover your new keypair:
width enhance concert vacant ketchup eternal spy craft spy guard tag punch    # If this was a real wallet, never share these words on the internet like this!
==========================================================================

$ solana airdrop 10 DYw8jCTfwHNRJhhmFcbXvVDTqWMEVFBX6ZKUmG5CNSKK --url https://devnet.solana.com  # Airdropping 10 SOL to my wallet's address/pubkey
Requesting airdrop of 10 SOL from 35.233.193.70:9900
10 SOL

$ solana balance DYw8jCTfwHNRJhhmFcbXvVDTqWMEVFBX6ZKUmG5CNSKK --url https://devnet.solana.com # Check the address's balance
10 SOL

$ solana-keygen new --no-outfile  # Creating a second wallet, a paper wallet
Generating a new keypair
For added security, enter a passphrase (empty for no passphrase):
====================================================================
pubkey: 7S3P4HxJpyyigGzodYwHtCxZyUQe9JiBMHyRWXArAaKv                   # Here is the address of the second, paper, wallet.
====================================================================
Save this seed phrase to recover your new keypair:
clump panic cousin hurt coast charge engage fall eager urge win love   # If this was a real wallet, never share these words on the internet like this!
====================================================================

$ solana transfer --from my_solana_wallet.json 7S3P4HxJpyyigGzodYwHtCxZyUQe9JiBMHyRWXArAaKv 5 --url https://devnet.solana.com --fee-payer my_solana_wallet.json  # Transferring tokens to the public address of the paper wallet
3gmXvykAd1nCQQ7MjosaHLf69Xyaqyq1qw2eu1mgPyYXd5G4v1rihhg1CiRw35b9fHzcftGKKEu4mbUeXY2pEX2z  # This is the transaction signature

$ solana balance DYw8jCTfwHNRJhhmFcbXvVDTqWMEVFBX6ZKUmG5CNSKK --url https://devnet.solana.com
4.999995 SOL  # The sending account has slightly less than 5 SOL remaining due to the 0.000005 SOL transaction fee payment

$ solana balance 7S3P4HxJpyyigGzodYwHtCxZyUQe9JiBMHyRWXArAaKv --url https://devnet.solana.com
5 SOL  # The second wallet has now received the 5 SOL transfer from the first wallet

```

## Receive Tokens

To receive tokens, you will need an address for others to send tokens to. In
Solana, the wallet address is the public key of a keypair. There are a variety
of techniques for generating keypairs. The method you choose will depend on how
you choose to store keypairs.  Keypairs are stored in wallets. Before receiving
tokens, you will need to [create a wallet](../wallet-guide/cli.md).
Once completed, you should have a public key
for each keypair you generated. The public key is a long string of base58
characters. Its length varies from 32 to 44 characters.

## Send Tokens

If you already hold SOL and want to send tokens to someone, you will need
a path to your keypair, their base58-encoded public key, and a number of
tokens to transfer. Once you have that collected, you can transfer tokens
with the `solana transfer` command:

```bash
solana transfer --from <KEYPAIR> <RECIPIENT_ACCOUNT_ADDRESS> <AMOUNT> --fee-payer <KEYPAIR>
```

Confirm the updated balances with `solana balance`:

```bash
solana balance <ACCOUNT_ADDRESS>
```
