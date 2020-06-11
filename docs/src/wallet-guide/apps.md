# App Wallets
Solana supports multiple third-party apps which should provide a familiar
experience for most people who are new or experienced with using crypto wallets.

## Trust Wallet
[Trust Wallet](https://trustwallet.com/) is an app for iOS and Android.  This is
currently the easiest and fastest way to get set up with a new wallet on Solana.
The app is free and getting your wallet set up only takes a few minutes.

### Trust Wallet Security

Tokens held in Trust Wallet are only as secure as the device on which the app is
installed.  Anyone who is able to unlock your phone or tablet may be able to
use the Trust Wallet app and transfer your tokens.  It is *highly* recommended
that you always use a strong passcode to lock your device and use a different
passcode to open the app itself.  Enable Trust Wallet passcode by opening
the app and going to Settings -> Security -> Passcode.

Additionally, anyone who has access to your seed phrase will be able to recreate
your Trust Wallet keys on a different device, and sign transactions from that
device, rather than on your own phone or tablet.
The seed phrase is displayed when a
new wallet is created, and it can also be viewed at any later time in the app by
going to Setting -> Wallets, and under the Options menu for a particular wallet,
tap "Show Recovery Phrase".  If you use an insecure method of electronic backup,
such as emailing yourself your seed phrase, if the email is intercepted or if
anyone else has access to the same email account, they would be able to create
your Trust Wallet keys on another device.

For these reasons, we do not recommend storing large quantities of SOL using
Trust Wallet.  For long term or cold storage of larger amounts of SOL, consider
using a Ledger Nano S, or a paper wallet.

{% page-ref page="trust-wallet.md" %}

## Ledger Live with Ledger Nano S
[Ledger Live](https://www.ledger.com/ledger-live) is available as free desktop
software and as a free app for iOS and Android.  It is used to manage apps and
crypto accounts on a Ledger *hardware wallet*, which must be purchased
separately and connected to the device running Ledger Live.

[Ledger Nano S](https://shop.ledger.com/products/ledger-nano-s) is a
hardware wallet which stores the wallet's private keys on a secure device that
is physically separate from the computer, and connects via USB cable.
This provides an extra level of security but requires the user to purchase and
keep track of the hardware device.

Solana does not support the Ledger Nano **X** at this time.

{% page-ref page="ledger-live.md" %}