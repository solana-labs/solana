#Ledger Live and Ledger Nano S
This document describes how to set up a
[Ledger Nano S hardware wallet](https://shop.ledger.com/products/ledger-nano-s)
with the [Ledger Live](https://www.ledger.com/ledger-live) software.

**NOTE: While Solana tools are fully integrated with the Ledger Nano S device,
and the Solana App can be installed on the Nano S using Ledger Live, adding and
managing wallet accounts currently requires use of our command-line tools.
Integration with Ledger Live to use Solana wallet accounts on Ledger Live
will be available in the future.**

Users may use a Ledger Nano S with the Solana command line tools.
Instructions for how to do that are found [here](../remote-wallet/ledger.md).

##Set up a Ledger Nano S
 - Order a Nano S from Ledger [here](https://shop.ledger.com/products/ledger-nano-s)
 - Follow the instructions for device setup included in the package,
 or [Ledger's Start page](https://www.ledger.com/start/)

##Install Ledger Live
 - Install [Ledger Live desktop software](https://www.ledger.com/ledger-live/),
 or
 - Install the Ledger Live app for [iOS](https://apps.apple.com/app/id1361671700)
 or [Android](https://play.google.com/store/apps/details?id=com.ledger.live).
   - Requires iOS 9.1 or later. Compatible with iPhone, iPad, and iPod touch.
   - Requires Android 7.0 or later.
 - Connect your Nano S to your device and follow the instructions

##Install the Solana App on your Nano S
 - Open Ledger Live
 - Go to Manager in the app and find "Solana (SOL)" in the App Catalog and
 click Install
   - Make sure your device is plugged in via USB and is unlocked with its PIN
 - You may be prompted on the Nano S to confirm the install of Solana App
 - "Solana (SOL)" should now show as Installed in the Ledger Live Manager

## Future Features (Not yet supported)

####Add a Solana account to Ledger Live
 - Make sure the Solana App is installed on the Nano S and the device is
 plugged in and unlocked with the PIN
 - In Ledger Live, go to Accounts, and click/tap Add Account and select
 "Solana (SOL)" from the crypto asset dropdown list
 - Follow the prompts on-screen to finish adding your Solana account

####Receiving SOL tokens
  - Open the Solana App on your Nano S
  - In Ledger Live, click on Accounts, and then select your Solana account
  - Click the Receive button
  - Select your Solana account
  - Your wallet receiving address should appear on screen.  You may copy this
   address and share it with anybody who wants to send you SOL tokens.
