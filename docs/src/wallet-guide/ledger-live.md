# Ledger Live and Ledger Nano S
This document describes how to set up a
[Ledger Nano S hardware wallet](https://shop.ledger.com/products/ledger-nano-s)
with the [Ledger Live](https://www.ledger.com/ledger-live) software.

**NOTE: While Solana tools are fully integrated with the Ledger Nano S device,
and the Solana App can be installed on the Nano S using Ledger Live, adding and
managing wallet accounts currently requires use of our command-line tools.
Integration with Ledger Live to use Solana wallet accounts on Ledger Live
will be available in the future.**

Users may [use a Ledger Nano S with the Solana command
line tools](../hardware-wallets/ledger.md).

## Set up a Ledger Nano S
 - Order a [Nano S from Ledger](https://shop.ledger.com/products/ledger-nano-s)
 - Follow the instructions for device setup included in the package,
 or [Ledger's Start page](https://www.ledger.com/start/)
- [Install the latest device firmware](https://support.ledgerwallet.com/hc/en-us/articles/360002731113-Update-Ledger-Nano-S-firmware)

## Install Ledger Live
 - Install [Ledger Live desktop software](https://www.ledger.com/ledger-live/),
 or
 - Install the [Ledger Live app for iOS](https://apps.apple.com/app/id1361671700)
 or [Ledger Live for Android](https://play.google.com/store/apps/details?id=com.ledger.live).
   - Requires iOS 9.1 or later. Compatible with iPhone, iPad, and iPod touch.
   - Requires Android 7.0 or later.
 - Connect your Nano S to your device and follow the instructions

## Install the Solana App on your Nano S
 - Open Ledger Live
 - Currently Ledger Live needs to be in "Developer Mode"
 (Settings > Experimental Features > Developer Mode) to see our app.

 ![Enabling Developer Mode](../.gitbook/assets/ledger-live-enable-developer-mode.png)

 - Go to Manager in the app and find "Solana" in the App Catalog and
 click Install
   - Make sure your device is plugged in via USB and is unlocked with its PIN
 - You may be prompted on the Nano S to confirm the install of Solana App
 - "Solana" should now show as "Installed" in the Ledger Live Manager

 ![Installed Solana App in Manager](../.gitbook/assets/ledger-live-install-solana-app.png)

## Interact with Solana network
- To interact with your Ledger wallet on our live network, please see our
instructions on how to
[use a Ledger Nano S with the Solana command line tools](../hardware-wallets/ledger.md).

## Support

Check out our [Wallet Support Page](support.md) for ways to get help.
