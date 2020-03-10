# Ledger Hardware Wallet

The Ledger Nano S hardware wallet offers secure storage of your Solana private
keys. The Solana Ledger app enables derivation of essentially infinite keys, and
secure transaction signing.

## Before You Begin

- [Install the Solana command-line tools](../install-solana.md)
- [Initialize your Ledger Nano S](https://support.ledger.com/hc/en-us/articles/360000613793)
- [Install the latest device firmware](https://support.ledgerwallet.com/hc/en-us/articles/360002731113-Update-Ledger-Nano-S-firmware)

## Install the Solana App on Ledger Nano S

The Solana Ledger app is not yet available on Ledger Live. Until it is, you
can install a development version of the app from the command-line. Note that
because the app is not installed via Ledger Live, you will need to approve
installation from an "unsafe" manager, as well as see the message, "This app
is not genuine" each time you open the app. Once the app is available on
Ledger Live, you can reinstall the app from there, and the message will no
longer be displayed.

1. Connect your Ledger device via USB and enter your pin to unlock it
2. Download and run the Solana Ledger app installer:
   ```text
   curl -sSLf https://github.com/solana-labs/ledger-app-solana/releases/download/v0.1.1/install.sh | sh
   ```
3. When prompted, approve the "unsafe" manager on your device
4. When prompted, approve the installation on your device
5. An installation window appears and your device will display Processing…
6. The app installation is confirmed

### Troubleshooting

If you encounter the following error:

```text
Traceback (most recent call last):
 File "/Library/Developer/CommandLineTools/Library/Frameworks/Python3.framework/Versions/3.7/lib/python3.7/runpy.py", line 193, in _run_module_as_main
  "__main__", mod_spec)
 File "/Library/Developer/CommandLineTools/Library/Frameworks/Python3.framework/Versions/3.7/lib/python3.7/runpy.py", line 85, in _run_code
  exec(code, run_globals)
 File "ledger-env/lib/python3.7/site-packages/ledgerblue/loadApp.py", line 197, in <module>
  dongle = getDongle(args.apdu)
 File "ledger-env/lib/python3.7/site-packages/ledgerblue/comm.py", line 216, in getDongle
  dev.open_path(hidDevicePath)
 File "hid.pyx", line 72, in hid.device.open_path
OSError: open failed
```

To fix, check the following:

1. Ensure your Ledger device is connected to USB
2. Ensure your Ledger device is unlocked and not waiting for you to enter your pin
3. Ensure the Ledger Live application is not open

### Future: Installation once the Solana app is on Ledger Live

- [Install Ledger Live](https://support.ledger.com/hc/en-us/articles/360006395553/) software on your computer

1. Open the Manager in Ledger Live
2. Connect your Ledger device via USB and enter your pin to unlock it
3. When prompted, approve the manager on your device
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

Configure the `solana` CLI tool to connect to a particular cluster:

```bash
solana config set --url <CLUSTER_URL> # (i.e. http://devnet.solana.com:8899)
```

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
