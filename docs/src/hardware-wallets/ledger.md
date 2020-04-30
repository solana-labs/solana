# Ledger Hardware Wallet

The Ledger Nano S hardware wallet offers secure storage of your Solana private
keys. The Solana Ledger app enables derivation of essentially infinite keys, and
secure transaction signing.

## Before You Begin

- [Set up a Ledger Nano S with the Solana App](../wallet-guide/ledger-live.md)
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

### Install the Solana Beta App

You're invited to help us test the latest pre-release version of our Ledger app
on one of the public testnets.

You can use the command-line to install the latest Solana Ledger app release before it has been validated by
the Ledger team and made available via Ledger Live.  Note that because the app
is not installed via Ledger Live, you will need to approve installation from an
"unsafe" manager, as well as see the message, "This app is not genuine" each
time you open the app. Once the app is available on Ledger Live, you can
reinstall the app from there, and the message will no longer be displayed.

**WARNING:** Installing an unsigned Ledger app reduces the security of your Ledger device.
If your client is compromised, an attacker will be able to trick you into signing arbitrary
transactions with arbitrary derivation paths. Only use this installation method if you understand
the security implications. We strongly recommend that you use a separate
Ledger device, with no other wallets/apps sharing the same seed phrase.

1. Connect your Ledger device via USB and enter your pin to unlock it
2. Download and run the Solana Ledger app installer:
   ```text
   curl -sSLf https://github.com/solana-labs/ledger-app-solana/releases/download/v0.2.0/install.sh | sh
   ```
3. When prompted, approve the "unsafe" manager on your device
4. When prompted, approve the installation on your device
5. An installation window appears and your device will display "Processing..."
6. The app installation is confirmed

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

## Support

Check out our [Wallet Support Page](../wallet-guide/support.md) for ways to get help.
