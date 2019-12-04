# Installation Guide
Follow this guide to setup Solana's key generation tool called `solana-keygen`

{% hint style="warn" %}
After installation, ensure your version is `0.21.1` or higher by running `solana-keygen -V`
{% endhint %}

First, download the desired release tarball from GitHub. The examples below will
retrieve the most recent release. If you would like to download a specific
version instead replace `latest/download` with `download/VERSION` where VERSION
is a tag name from https://github.com/solana-labs/solana/releases (ie. v0.21.1).

MacOS
```bash
$ curl -L -sSf -o solana-release.tar.bz2 'https://github.com/solana-labs/solana/releases/latest/download/solana-release-x86_64-apple-darwin.tar.bz2'
```
**`'https://github.com/solana-labs/solana/releases/latest/download/solana-release-x86_64-apple-darwin.tar.bz2'`**

Linux
```bash
$ curl -L -sSf -o solana-release.tar.bz2 'https://github.com/solana-labs/solana/releases/latest/download/solana-release-x86_64-unknown-linux-gnu.tar.bz2'
```
**`'https://github.com/solana-labs/solana/releases/latest/download/solana-release-x86_64-unknown-linux-gnu.tar.bz2'`**

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
