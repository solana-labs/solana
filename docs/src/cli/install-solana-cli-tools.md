# Install the Solana Tool Suite

There are multiple ways to install the Solana tools on your computer
depending on your preferred workflow:
 - [Use Solana's Install Tool (Simplest option)](#use-solanas-install-tool)
 - [Download Prebuilt Binaries](#download-prebuilt-binaries)
 - [Build from Source](#build-from-source)

## Use Solana's Install Tool

### MacOS & Linux

 - Open your favorite Terminal application

 - Install the Solana release
[LATEST_SOLANA_RELEASE_VERSION](https://github.com/solana-labs/solana/releases/tag/LATEST_SOLANA_RELEASE_VERSION) on your
machine by running:

```bash
curl -sSf https://raw.githubusercontent.com/solana-labs/solana/LATEST_SOLANA_RELEASE_VERSION/install/solana-install-init.sh | sh -s - LATEST_SOLANA_RELEASE_VERSION
```

 - If you are connecting to a different testnet, you can replace `LATEST_SOLANA_RELEASE_VERSION` with the
release tag matching the software version of your desired testnet, or replace it
with the named channel `stable`, `beta`, or `edge`.

- The following output indicates a successful update:

```text
looking for latest release
downloading LATEST_SOLANA_RELEASE_VERSION installer
Configuration: /home/solana/.config/solana/install/config.yml
Active release directory: /home/solana/.local/share/solana/install/active_release
* Release version: LATEST_SOLANA_RELEASE_VERSION
* Release URL: https://github.com/solana-labs/solana/releases/download/LATEST_SOLANA_RELEASE_VERSION/solana-release-x86_64-unknown-linux-gnu.tar.bz2
Update successful
```

 - Depending on your system, the end of the installer messaging may prompt you
 to
 ```bash
Please update your PATH environment variable to include the solana programs:
```
 - If you get the above message, copy and paste the recommended command below
   it to update `PATH`
 - Confirm you have the desired version of `solana` installed by running:
 ```bash
 solana --version
```

 - After a successful install, `solana-install update` may be used to easily
update the Solana software to a newer version at any time.

***

###Windows

 - Open a Command Prompt (`cmd.exe`) as an Administrator
   - Search for Command Prompt in the Windows search bar.  When the Command
   Prompt app appears, right-click and select “Open as Administrator”.
If you are prompted by a pop-up window asking “Do you want to allow this app to
make changes to your device?”, click Yes.

 - Copy and paste the following command, then press Enter to download the Solana
 installer into a temporary directory:

```bash
curl http://release.solana.com/LATEST_SOLANA_RELEASE_VERSION/solana-install-init-x86_64-pc-windows-gnu.exe --output C:\solana-install-tmp\solana-install-init.exe --create-dirs
```

 - Copy and paste the following command, then press Enter to install the latest
 version of Solana.  If you see a security pop-up by your system, please select
 to allow the program to run.

```bash
C:\solana-install-tmp\solana-install-init.exe LATEST_SOLANA_RELEASE_VERSION
```

 - When the installer is finished, press Enter.

 - Close the command prompt window and re-open a new command prompt window as a
normal user
   - Search for "Command Prompt" in the search bar, then left click on the
Command Prompt app icon, no need to run as Administrator)
 - Confirm you have the desired version of `solana` installed by entering:
 ```bash
 solana --version
```

 - After a successful install, `solana-install update` may be used to easily
update the Solana software to a newer version at any time.

## Download Prebuilt Binaries

If you would rather not use `solana-install` to manage the install, you can
manually download and install the binaries.

### Linux

Download the binaries by navigating to
[https://github.com/solana-labs/solana/releases/latest](https://github.com/solana-labs/solana/releases/latest),
download **solana-release-x86\_64-unknown-linux-gnu.tar.bz2**, then extract the
archive:

```bash
tar jxf solana-release-x86_64-unknown-linux-gnu.tar.bz2
cd solana-release/
export PATH=$PWD/bin:$PATH
```

### MacOS

Download the binaries by navigating to
[https://github.com/solana-labs/solana/releases/latest](https://github.com/solana-labs/solana/releases/latest),
download **solana-release-x86\_64-apple-darwin.tar.bz2**, then extract the
archive:

```bash
tar jxf solana-release-x86_64-apple-darwin.tar.bz2
cd solana-release/
export PATH=$PWD/bin:$PATH
```

### Windows

- Download the binaries by navigating to
[https://github.com/solana-labs/solana/releases/latest](https://github.com/solana-labs/solana/releases/latest),
download **solana-release-x86\_64-pc-windows-gnu.tar.bz2**, then extract the
archive using WinZip or similar.

- Open a Command Prompt and navigate to the directory into which you extracted
the binaries and run:

```bash
cd solana-release/
set PATH=%cd%/bin;%PATH%
```

## Build From Source

If you are unable to use the prebuilt binaries or prefer to build it yourself
from source, navigate to
[https://github.com/solana-labs/solana/releases/latest](https://github.com/solana-labs/solana/releases/latest),
and download the **Source Code** archive. Extract the code and build the
binaries with:

```bash
./scripts/cargo-install-all.sh .
export PATH=$PWD/bin:$PATH
```

You can then run the following command to obtain the same result as with
prebuilt binaries:

```bash
solana-install init
```

# Choosing a Cluster

Solana maintains several clusters, each featuring a Solana-owned validator
that serves as an entrypoint to the cluster.

Current cluster entrypoints:

* Devnet: devnet.solana.com
* Tour de SOL: tds.solana.com

Application developers should target Devnet. Key differences
between Devnet and what will be Mainnet:

* Devnet tokens are not real
* Devnet includes a token faucet for application testing
* Devnet may be subject to ledger resets
* Devnet typically runs a newer software version than mainnet
* Devnet may be maintained by different validators than mainnet

## Configure the Command-line

You can check what cluster the Solana CLI is currently targeting by
running the following command:

```bash
solana config get
```

Use the `solana config set` command to target a different cluster.
For example, for Devnet, use:

```bash
solana config set --url http://devnet.solana.com
```

## Ensure Versions Match

Though not strictly necessary, the CLI will generally work best when its version
matches the software version running on the cluster. To get the CLI version, run:

```bash
solana --version
```

To get the cluster version, run:

```bash
solana cluster-version
```

Ensure the CLI version is greater than or equal to the cluster version.
