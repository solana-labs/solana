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
solana-keygen pubkey usb://ledger
```

This confirms your Ledger device is connected properly and in the correct state
to interact with the Solana CLI. The command returns your Ledger's unique
*wallet ID*. When you have multiple Nano S devices connected to the same
computer, you can use your wallet key to specify which Ledger hardware wallet
you want to use. Run the same command again, but this time, with its fully
qualified URL:

```text
solana-keygen pubkey usb://ledger/<WALLET_ID>
```

where you replace `<WALLET_ID>` with the output of the first command.
Confirm it prints the same wallet ID as before.

To learn more about keypair URLs, see
[Specify A Hardware Wallet Key](README.md#specify-a-hardware-wallet-key)

Read more about [sending and receiving tokens](../transfer-tokens.md) and
[delegating stake](../delegate-stake.md). You can use your Ledger keypair URL
anywhere you see an option or argument that accepts a `<KEYPAIR>`.

## Support

Email maintainers@solana.com
