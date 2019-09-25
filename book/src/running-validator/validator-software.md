# Installing the Validator Software

## Bootstrap with `solana-install`

The `solana-install` tool can be used to easily install and upgrade the validator software on Linux x86\_64 and mac OS systems.

```bash
$ curl -sSf https://raw.githubusercontent.com/solana-labs/solana/v0.18.0/install/solana-install-init.sh | sh -s
```

Alternatively build the `solana-install` program from source and run the following command to obtain the same result:

```bash
$ solana-install init
```

After a successful install, `solana-install update` may be used to easily update the cluster software to a newer version at any time.

## Download Prebuilt Binaries

If you would rather not use `solana-install` to manage the install, you can manually download and install the binaries.

### Linux

Download the binaries by navigating to [https://github.com/solana-labs/solana/releases/latest](https://github.com/solana-labs/solana/releases/latest), download **solana-release-x86\_64-unknown-linux-gnu.tar.bz2**, then extract the archive:

```bash
$ tar jxf solana-release-x86_64-unknown-linux-gnu.tar.bz2
$ cd solana-release/
$ export PATH=$PWD/bin:$PATH
```

### mac OS

Download the binaries by navigating to [https://github.com/solana-labs/solana/releases/latest](https://github.com/solana-labs/solana/releases/latest), download **solana-release-x86\_64-apple-darwin.tar.bz2**, then extract the archive:

```bash
$ tar jxf solana-release-x86_64-apple-darwin.tar.bz2
$ cd solana-release/
$ export PATH=$PWD/bin:$PATH
```

## Build From Source

If you are unable to use the prebuilt binaries or prefer to build it yourself from source, navigate to [https://github.com/solana-labs/solana/releases/latest](https://github.com/solana-labs/solana/releases/latest), and download the **Source Code** archive. Extract the code and build the binaries with:

```bash
$ ./scripts/cargo-install-all.sh .
$ export PATH=$PWD/bin:$PATH
```
