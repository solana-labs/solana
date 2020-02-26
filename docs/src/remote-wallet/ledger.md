# Ledger Hardware Wallet

The Ledger Nano S hardware wallet offers secure storage of your Solana private
keys. The Solana ledger app enables derivation of essentially infinite keys, and
secure signing of messages.

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
```
solana address --keypair usb://ledger
```
This confirms your Ledger device is connected properly and in the correct state to interact with the Solana CLI.

### Ledger Device Paths

Solana uses a standardized path to identify remote wallets and desired key. This
path looks like:
```
usb://ledger/nano-s/<WALLET_BASE_KEY>?key=<DERIVATION>
```

**Something about how only "usb://ledger" is necessary for a single attached Ledger**

Your Ledger's limitless keys are indexed as per [BIP44 specifications](https://github.com/bitcoin/bips/blob/master/bip-0044.mediawiki).
Solana keys reside at derivation path `44'/501'`, and Solana supports two levels
of key path.

Add a desired key derivation to a remote-wallet path using the query string
`?key=<DERIVATION>`, with derivation formatted as `<ACCOUNT>/<CHANGE>`.

For example, a complete Ledger device path might be:
```
usb://ledger/nano-s/BsNsvfXqQTtJnagwFWdBS7FBXgnsK8VZ5CmuznN85swK?key=0/0
```

### Set CLI Configuration

If you want to you a Ledger key as the default signer for CLI commands, use the
[CLI configuration settings](../cli/usage.md#solana-config):
```
solana config set --keypair <LEDGER PATH>
```

For example:
```
solana config set --keypair usb://ledger?key=1/2
```

### Check Account Balance

```
solana balance --keypair usb://ledger?key=12345
```

### Send SOL via Ledger Device
```
solana transfer <RECIPIENT> <AMOUNT> --from <LEDGER PATH>
```

### Delegate Stake with Ledger Device

## Support
