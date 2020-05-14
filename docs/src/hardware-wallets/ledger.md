# Ledger Hardware Wallet

The Ledger Nano S hardware wallet offers secure storage of your Solana private
keys. The Solana Ledger app enables derivation of essentially infinite keys, and
secure transaction signing.

## Before You Begin

- [Set up a Ledger Nano S with the Solana App](../wallet-guide/ledger-live.md)
- [Install the Solana command-line tools](../cli/install-solana-cli-tools.md)

## Use Ledger Nano S with Solana CLI

1. Ensure the Ledger Live application is closed
2. Plug your Ledger device into your computer's USB port
3. Enter your pin and start the Solana app on the Ledger device
4. Press both buttons to advance past the "Pending Ledger review" screen
5. Ensure the screen reads "Application is ready"

### View your Wallet ID

On your computer, run:

```bash
solana-keygen pubkey usb://ledger
```

This confirms your Ledger device is connected properly and in the correct state
to interact with the Solana CLI. The command returns your Ledger's unique
*wallet ID*. When you have multiple Nano S devices connected to the same
computer, you can use your wallet ID to specify which Ledger hardware wallet
you want to use.  If you only plan to use a single Nano S on your computer
at a time, you don't need to include the wallet ID.  For information on
using the wallet ID to use a specific Ledger, see
[Manage Multiple Hardware Wallets](#manage-multiple-hardware-wallets).

### View your Wallet Addresses

Your Nano S supports an arbitrary number of valid wallet addresses and signers.
To view any address, use the `solana-keygen pubkey` command, as shown below,
followed by a valid [keypair URL](README.md#specify-a-keypair-url).

Multiple wallet addresses can be useful if you want to transfer tokens between
your own accounts for different purposes, or use different keypairs on the
device as signing authorities for a stake account, for example.

All of the following commands will display different addresses, associated with
the keypair path given.  Try them out!

```bash
solana-keygen pubkey usb://ledger
solana-keygen pubkey usb://ledger?key=0
solana-keygen pubkey usb://ledger?key=1
solana-keygen pubkey usb://ledger?key=2
```

You can use other values for the number after `key=` as well.
Any of the addresses displayed by these commands are valid Solana wallet
addresses. The private portion associated with each address is stored securely
on the Nano S device, and is used to sign transactions from this address.
Just make a note of which keypair URL you used to derive any address you will be
using to receive tokens.

If you are only planning to use a single address/keypair on your device, a good
easy-to-remember path might be to use the address at `key=0`.  View this address
with:
```bash
solana-keygen pubkey usb://ledger?key=0
```

Now you have a wallet address (or multiple addresses), you can share any of
these addresses publicly to act as a receiving address, and you can use the
associated keypair URL as the signer for transactions from that address.

### View your Balance

To view the balance of any account, regardless of which wallet it uses, use the
`solana balance` command:
```bash
solana balance SOME_WALLET_ADDRESS
```

For example, if your address is `7cvkjYAkUYs4W8XcXsca7cBrEGFeSUjeZmKoNBvEwyri`,
then enter the following command to view the balance:
```bash
solana balance 7cvkjYAkUYs4W8XcXsca7cBrEGFeSUjeZmKoNBvEwyri
```

You can also view the balance of any account address on the Accounts tab in the
[Explorer](https://explorer.solana.com/accounts)
and paste the address in the box to view the balance in you web browser.

Note: Any address with a balance of 0 SOL, such as a newly created one on your
Ledger, will show as "Not Found" in the explorer.  Empty accounts and non-existent
accounts are treated the same in Solana.  This will change when your account
address has some SOL in it.

### Send SOL from a Ledger Nano S

To send some tokens from an address controlled by your Nano S device, you will
need to use the device to sign a transaction, using the same keypair URL you
used to derive the address.  To do this, make sure your Nano S is plugged in,
unlocked with the PIN, Ledger Live is not running, and the Solana App is open
on the device, showing "Application is Ready".

The `solana transfer` command is used to specify to which address to send tokens,
how many tokens to send, and uses the `--keypair` argument to specify which
keypair is sending the tokens, which will sign the transaction, and the balance
from the associated address will decrease.

```bash
solana transfer RECIPIENT_ADDRESS AMOUNT --keypair KEYPAIR_URL_OF_SENDER
```

Below is a full example.  First, an address is viewed at a certain keypair URL.
Second, the balance of tht address is checked.  Lastly, a transfer transaction
is entered to send `1` SOL to the recipient address `7cvkjYAkUYs4W8XcXsca7cBrEGFeSUjeZmKoNBvEwyri`.
When you hit Enter for a transfer command, you will be prompted to approve the
transaction details on your Ledger device.  On the device, use the right and
left buttons to review the transaction details.  If they look correct, click
both buttons on the "Approve" screen, otherwise push both buttons on the "Reject"
screen.

```bash
~$ solana-keygen pubkey usb://ledger?key=42
CjeqzArkZt6xwdnZ9NZSf8D1CNJN1rjeFiyd8q7iLWAV

~$ solana balance CjeqzArkZt6xwdnZ9NZSf8D1CNJN1rjeFiyd8q7iLWAV
1.000005 SOL

~$ solana transfer 7cvkjYAkUYs4W8XcXsca7cBrEGFeSUjeZmKoNBvEwyri 1 --keypair usb://ledger?key=42
Waiting for your approval on Ledger hardware wallet usb://ledger/2JT2Xvy6T8hSmT8g6WdeDbHUgoeGdj6bE2VueCZUJmyN
✅ Approved

Signature: kemu9jDEuPirKNRKiHan7ycybYsZp7pFefAdvWZRq5VRHCLgXTXaFVw3pfh87MQcWX4kQY4TjSBmESrwMApom1V
```

After approving the transaction on your device, the program will display the
transaction signature, and wait for the maximum number of confirmations (32)
before returning.  This only takes a few seconds, and then the transaction is
finalized on the Solana network.  You can view details of this or any other
transaction by going to the Transaction tab in the
[Explorer](https://explorer.solana.com/transactions)
and paste in the transaction signature.

## Advanced Operations

### Manage Multiple Hardware Wallets

It is sometimes useful to sign a transaction with keys from multiple hardware
wallets. Signing with multiple wallets requires *fully qualified keypair URLs*.
When the URL is not fully qualified, the Solana CLI will prompt you with
the fully qualified URLs of all connected hardware wallets, and ask you to
choose which wallet to use for each signature.

Instead of using the interactive prompts, you can generate fully qualified
URLs using the Solana CLI `resolve-signer` command. For example, try
connecting a Ledger Nano-S to USB, unlock it with your pin, and running the
following command:

```text
solana resolve-signer usb://ledger?key=0/0
```

You will see output similar to:

```text
usb://ledger/BsNsvfXqQTtJnagwFWdBS7FBXgnsK8VZ5CmuznN85swK?key=0/0
```

but where `BsNsvfXqQTtJnagwFWdBS7FBXgnsK8VZ5CmuznN85swK` is your `WALLET_ID`.

With your fully qualified URL, you can connect multiple hardware wallets to
the same computer and uniquely identify a keypair from any of them.
Use the output from the `resolve-signer` command anywhere a `solana` command
expects a `<KEYPAIR>` entry to use that resolved path as the signer for that
part of the given transaction.

### Install the Solana Beta App

You're invited to help us test the latest pre-release version of our Ledger app
on one of the public testnets.

You can use the command-line to install the latest Solana Ledger app release
before it has been validated by
the Ledger team and made available via Ledger Live.  Note that because the app
is not installed via Ledger Live, you will need to approve installation from an
"unsafe" manager, as well as see the message, "This app is not genuine" each
time you open the app. Once the app is available on Ledger Live, you can
reinstall the app from there, and the message will no longer be displayed.

**WARNING:** Installing an unsigned Ledger app reduces the security of your
Ledger device.
If your client is compromised, an attacker will be able to trick you into
signing arbitrary transactions with arbitrary derivation paths.
Only use this installation method if you understand
the security implications. We strongly recommend that you use a separate
Ledger device, with no other wallets/apps sharing the same seed phrase.

1. Connect your Ledger device via USB and enter your pin to unlock it
2. Download and run the Solana Ledger app installer:
   ```text
   curl -sSLf https://github.com/solana-labs/ledger-app-solana/releases/download/v0.2.1/install.sh | sh
   ```
3. When prompted, approve the "unsafe" manager on your device
4. When prompted, approve the installation on your device
5. An installation window appears and your device will display "Processing..."
6. The app installation is confirmed

#### Installing the Solana Beta App returns an error

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

## Troubleshooting

### Keypair URL parameters are ignored in zsh

The question mark character is a special character in zsh. If that's not a
feature you use, add the following line to your `~/.zshrc` to treat it as a
normal character:

```bash
unsetopt nomatch
```

Then either restart your shell window or run `~/.zshrc`:

```bash
source ~/.zshrc
```

If you would prefer not to disable zsh's special handling of the question mark
character, you can disable it explictly with a backslash in your keypair URLs.
For example:

```bash
solana-keygen pubkey usb://ledger\?key=0
```

## Support

Check out our [Wallet Support Page](../wallet-guide/support.md)
for ways to get help.




Read more about [sending and receiving tokens](../cli/transfer-tokens.md) and
[delegating stake](../cli/delegate-stake.md). You can use your Ledger keypair URL
anywhere you see an option or argument that accepts a `<KEYPAIR>`.