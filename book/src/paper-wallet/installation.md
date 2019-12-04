# Installation
Solana provides a CLI tool for key generation called `solana-keygen`. Here are 3
different options for installations.

## Option 1: via Solana Install Script
Install the Solana CLI tools on your machine by running:

```bash
curl -sSf https://raw.githubusercontent.com/solana-labs/solana/v0.21.1/install/solana-install-init.sh | sh -s - 0.21.1
```

The following output indicates a successful update:

```text
looking for latest release
downloading v0.21.1 installer
Configuration: /home/solana/.config/solana/install/config.yml
Active release directory: /home/solana/.local/share/solana/install/active_release
* Release version: 0.21.1
* Release URL: https://github.com/solana-labs/solana/releases/download/v0.21.1/solana-release-x86_64-unknown-linux-gnu.tar.bz2
Update successful
```

You can now verify that `solana-keygen` was installed by running:

```bash
solana-keygen -h
```

After a successful install, `solana-install update` may be used to easily update
the cluster software to a newer version at any time.

### Option 2: via Rust Cargo
First, install Rust's package manager `cargo`

```bash
$ curl https://sh.rustup.rs -sSf | sh
$ source $HOME/.cargo/env
```

Then, install the `solana-keygen` tool

```bash
cargo install solana-keygen
```

You can now verify that `solana-keygen` was installed by running:

```bash
solana-keygen -h
```

### Option 3: via Tarball
First download the desired release tarball from GitHub. The examples below will
retrieve the most recent release. If you would like to download a specific
version instead replace `latest/download` with `download/VERSION` where VERSION
is a tag name from https://github.com/solana-labs/solana/releases (ie. v0.21.0).

MacOS
```bash
$ curl -L -sSf -o solana-release.tar.bz2 'https://github.com/solana-labs/solana/releases/latest/download/solana-release-x86_64-apple-darwin.tar.bz2'
```

Linux
```bash
$ curl -L -sSf -o solana-release.tar.bz2 'https://github.com/solana-labs/solana/releases/latest/download/solana-release-x86_64-unknown-linux-gnu.tar.bz2'
```

Next, extract the tarball
```bash
$ tar xf solana-release.tar.bz2
```

Finally, `solana-keygen` can be run by
```bash
$ solana-release/bin/solana-keygen
```

If you would like to follow the remainder of these instructions without typing
the leading path to `solana-keygen`, add it to your PATH environment variable
with the following command
```bash
$ export PATH="$(pwd)/solana-release/bin:${PATH}"
```
This can be made permanent by adding it to your `~/.profile`
