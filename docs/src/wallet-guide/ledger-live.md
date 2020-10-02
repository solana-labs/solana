---
title: Ledger Live and Ledger Nano S
---

This document describes how to set up a
[Ledger Nano S hardware wallet](https://shop.ledger.com/products/ledger-nano-s)
with the [Ledger Live](https://www.ledger.com/ledger-live) software.

Once the setup steps shown below are complete and the Solana app is installed
on your Nano S device, users have several options of how to
[use the Nano S to interact with the Solana Network](#interact-with-the-solana-network)

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
- Go to Manager in the app and find "Solana" in the App Catalog and
  click Install
  - Make sure your device is plugged in via USB and is unlocked with its PIN
- You may be prompted on the Nano S to confirm the install of Solana App
- "Solana" should now show as "Installed" in the Ledger Live Manager

![Installed Solana App in Manager](/img/ledger-live-latest-version-installed.png)

## Upgrade to the latest version of the Solana App

To make sure you have the latest functionality, if you are using an older version
of the Solana App, please upgrade to version v0.2.2 by following these steps.

- Connect your Nano S to your computer an unlock it by entering your PIN on the
  device
- Open Ledger Live and click on "Manager" in the left pane
- On your Nano S, click both buttons when prompted to "Allow Manager"
- Click the "Update All" button to update the Solana app to the latest version
  (v.0.2.2)

![Upgrade All button in Manager](/img/ledger-live-update-available-v0.2.2.png)

- Once the upgrade is finished, confirm v0.2.2 is installed under "Apps Installed"

![Upgrade complete](/img/ledger-live-latest-version-installed.png)

## Interact with the Solana network

Users can use any of the following options to sign and submit transactions with
the Ledger Nano S to interact with the Solana network:

- [SolFlare.com](https://solflare.com/) is a non-custodial web wallet built
specifically for Solana and supports basic transfers and staking operations
with the Ledger device.
Check out our guide for [using a Ledger Nano S with SolFlare](solflare.md).

- Developers and advanced users may
[use a Ledger Nano S with the Solana command line tools](hardware-wallets/ledger.md).
New wallet features are almost always supported in the native command line tools
before being supported by third-party wallets.

## Support

Check out our [Wallet Support Page](support.md) for ways to get help.
