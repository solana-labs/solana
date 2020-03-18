# Send and Receive Tokens

## Receive Tokens

To receive tokens, you will need an address for others to send tokens to. In
Solana, an address is the public key of a keypair. There are a variety
of techniques for generating keypairs. The method you choose will depend on how
you choose to store keypairs.  Keypairs are stored in wallets. Before receiving
tokens, you'll need to [choose a wallet](choose-a-wallet.md) and
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

The command's usage message will typically contain words such as `<NUMBER>`,
`<PUBKEY>` or `<KEYPAIR>`. Each word is a placeholder for the *type* of text
you can execute the command with. For example, you can replace `<NUMBER>`
with a number such as `42` or `100.42`. You can replace `<PUBKEY>` with the
base58 encoding of your public key. For `<KEYPAIR>`, it depends on what type
of wallet you chose. If you chose an fs wallet, that path might be
`~/my-solana-wallet/my-keypair.json`.  If you chose a paper wallet, use the
keyword `ASK`, and the Solana CLI will prompt you for your seed phrase. If
you chose a hardware wallet, use your USB URL, such as `usb://ledger?key=0`.

### Test-drive your Public Keys

Before sharing your public key with others, you may want to first ensure the
key is valid and that you indeed hold the corresponding private key.

Try and *airdrop* yourself some play tokens on the developer testnet, called
Devnet:

```bash
solana airdrop 10 <PUBKEY> --url http://devnet.solana.com
```

where you replace the text `<PUBKEY>` with your base58 public key.

Confirm the airdrop was successful by checking the account's balance.
It should output `10 SOL`:

```bash
solana balance <PUBKEY> --url http://devnet.solana.com
```

Next, prove that you own those tokens by transferring them. The Solana cluster
will only accept the transfer if you sign the transaction with the private
key corresponding to the sender's public key in the transaction.

First, we'll need a public key to receive our tokens. Create a second
keypair and record its pubkey:

```bash
solana-keygen new --no-passphrase --no-outfile
```

The output will contain the public key after the text `pubkey:`. Copy the
public key. We'll use it in the next step.

```text
============================================================================
pubkey: GKvqsuNcnwWqPzzuhLmGi4rzzh55FhJtGizkhHaEJqiV
============================================================================
```

```bash
solana transfer --keypair=<KEYPAIR> <PUBKEY> 5 --url http://devnet.solana.com
```

where you replace `<KEYPAIR>` with the path to a keypair in your wallet,
and replace `<PUBKEY>` with the output of `solana-keygen new` above.

Confirm the updated balances with `solana balance`:

```bash
solana balance <PUBKEY> --url http://devnet.solana.com
```

where `<PUBKEY>` is either the public key from your keypair or the
recipient's public key.

## Send Tokens

If you already hold SOL and want to send tokens to someone, you will need
a path to your keypair, their base58-encoded public key, and a number of
tokens to transfer. Once you have that collected, you can transfer tokens
with the `solana transfer` command:

```bash
solana transfer --keypair=<KEYPAIR> <PUBKEY> <NUMBER>
```

Confirm the updated balances with `solana balance`:

```bash
solana balance <PUBKEY>
```
