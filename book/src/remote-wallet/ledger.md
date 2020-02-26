# Ledger Hardware Wallet

The Ledger Nano S hardware wallet offers secure storage of your Solana private
keys. The Solana Ledger app enables derivation of essentially infinite keys, and
secure transaction signing.

## Before You Begin

- [Initialize your Ledger Nano S](https://support.ledger.com/hc/en-us/articles/360000613793)
- [Install the latest device firmware](https://support.ledgerwallet.com/hc/en-us/articles/360002731113-Update-Ledger-Nano-S-firmware)
- [Install Ledger Live](https://support.ledger.com/hc/en-us/articles/360006395553/) software on your computer

## Install the Solana App on Ledger Nano S

1. Open the Manager in Ledger Live
2. Connect and unlock your Ledger Nano S
3. If asked, allow the manager on your device by pressing the right button
4. Find Solana in the app catalog and click Install
5. An installation window appears and your device will display Processing…
6. The app installation is confirmed

## Use Ledger Device with Solana CLI

1. Plug your Ledger device into your computer's USB port
2. Enter your pin and start the Solana app on the Ledger device
3. On your computer, run:

```text
solana address --keypair usb://ledger
```

This confirms your Ledger device is connected properly and in the correct state
to interact with the Solana CLI. The command returns your Ledger's unique
*wallet key*. When you have multiple Nano S devices connected to the same
computer, you can use your wallet key to specify which Ledger hardware wallet
you want to use. Run the same command again, but this time, with its fully
qualified URL:

```text
solana address --keypair usb://ledger/nano-s/<WALLET_KEY>
```

Confirm it prints the same key as when you entered just `usb://ledger`.

### Ledger Device URLs

Solana defines a format for the URL protocol "usb://" to uniquely locate any Solana key on
any remote wallet connected to your computer.

The URL has the form, where square brackets denote optional fields:

```text
usb://ledger[/<LEDGER_TYPE>[/<WALLET_KEY>]][?key=<DERIVATION_PATH>]
```

`LEDGER_TYPE` is optional and defaults to the value "nano-s". If the value is provided,
it must be "nano-s" without quotes, the only supported Ledger device at this time.

`WALLET_KEY` is used to disambiguate multiple Nano S devices. Every Ledger has
a unique master key and from that key derives a separate unique key per app.

`DERVIATION_PATH` is used to navigate to Solana keys within your Ledger hardware
wallet. The path has the form `<ACCOUNT>[/<CHANGE>]`, where each `ACCOUNT` and
`CHANGE` are postive integers.

All derivation paths implicitly include the prefix `44'/501'`, which indicates
the path follows the [BIP44 specifications](https://github.com/bitcoin/bips/blob/master/bip-0044.mediawiki)
and that any dervied keys are Solana keys (Coin type 501).  The single quote
indicates a "hardened" derivation. Because Solana uses Ed25519 keypairs, all
derivations are hardened and therefore adding the quote is optional and
unnecessary.

For example, a complete Ledger device path might be:

```text
usb://ledger/nano-s/BsNsvfXqQTtJnagwFWdBS7FBXgnsK8VZ5CmuznN85swK?key=0/0
```

### Set CLI Configuration

If you want to set a Ledger key as the default signer for CLI commands, use the
[CLI configuration settings](../cli/usage.md#solana-config):

```text
solana config set --keypair <LEDGER_URL>
```

For example:

```text
solana config set --keypair usb://ledger?key=0
```

### Check Account Balance

```text
solana balance --keypair usb://ledger?key=12345
```

Or with the default signer:

```text
solana balance
```

### Send SOL via Ledger Device

```text
solana transfer <RECIPIENT> <AMOUNT> --from <LEDGER_URL>
```

Or with the default signer:

```text
solana transfer <RECIPIENT> <AMOUNT>
```

### Delegate Stake with Ledger Device

```text
solana delegate-stake <STAKE_ACCOUNT> <VOTER_ID> --keypair <LEDGER_URL>
```

Or with the default signer:

```text
solana delegate-stake <STAKE_ACCOUNT> <VOTER_ID>
```

## Support

Email maintainers@solana.com
