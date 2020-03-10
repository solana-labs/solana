# Install the Solana Tool Suite

Install the Solana release
[v1.1.0](https://github.com/solana-labs/solana/releases/tag/v1.1.0) on your
machine by running:

```bash
curl -sSf https://raw.githubusercontent.com/solana-labs/solana/v1.1.0/install/solana-install-init.sh | sh -s - 1.1.0
```

If you are connecting to a different testnet, you can replace `1.1.0` with the
release tag matching the software version of your desired testnet, or replace it
with the named channel `stable`, `beta`, or `edge`.

The following output indicates a successful update:

```text
looking for latest release
downloading v1.1.0 installer
Configuration: /home/solana/.config/solana/install/config.yml
Active release directory: /home/solana/.local/share/solana/install/active_release
* Release version: 1.1.0
* Release URL: https://github.com/solana-labs/solana/releases/download/v1.1.0/solana-release-x86_64-unknown-linux-gnu.tar.bz2
Update successful
```

After a successful install, `solana-install update` may be used to easily update
the cluster software to a newer version at any time.

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

### macOS

Download the binaries by navigating to
[https://github.com/solana-labs/solana/releases/latest](https://github.com/solana-labs/solana/releases/latest),
download **solana-release-x86\_64-apple-darwin.tar.bz2**, then extract the
archive:

```bash
tar jxf solana-release-x86_64-apple-darwin.tar.bz2
cd solana-release/
export PATH=$PWD/bin:$PATH
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
