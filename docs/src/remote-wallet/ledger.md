# Ledger Hardware Wallet

The Ledger Nano S hardware wallet offers secure storage of your Solana private
keys. The Solana Ledger app enables derivation of essentially infinite keys, and
secure transaction signing.

## Before You Begin

- [Set up a Ledger Nano S with the Solana App](../wallet/ledger-live.md)
- [Install the Solana command-line tools](../cli/install-solana-cli-tools.md)

## Use Ledger Device with Solana CLI

1. Ensure the Ledger Live application is closed
2. Plug your Ledger device into your computer's USB port
3. Enter your pin and start the Solana app on the Ledger device
4. Press both buttons to advance past the "Pending Ledger review" screen
5. Ensure the screen reads "Application is ready"
6. On your computer, run:

```bash
solana-keygen pubkey usb://ledger
```

This confirms your Ledger device is connected properly and in the correct state
to interact with the Solana CLI. The command returns your Ledger's unique
*wallet ID*. When you have multiple Nano S devices connected to the same
computer, you can use your wallet key to specify which Ledger hardware wallet
you want to use. Run the same command again, but this time, with its fully
qualified URL:

```bash
solana-keygen pubkey usb://ledger/<WALLET_ID>
```

where you replace `<WALLET_ID>` with the output of the first command.
Confirm it prints the same wallet ID as before.

To learn more about keypair URLs, see
[Specify A Hardware Wallet Key](README.md#specify-a-hardware-wallet-key)

Read more about [sending and receiving tokens](../cli/transfer-tokens.md) and
[delegating stake](../cli/delegate-stake.md). You can use your Ledger keypair URL
anywhere you see an option or argument that accepts a `<KEYPAIR>`.

## Support

Check out our [Wallet Support Page](../wallet/support.md) for ways to get help.
