# Installation Guide
Follow this guide to setup Solana's key generation tool called `solana-keygen`

{% hint style="warn" %}
After installation, ensure your version is `0.21.1` or higher by running `solana-keygen -V`
{% endhint %}

## Download
First, download the latest release tarball from GitHub.

1. Setup download url

  ```bash
  solana_downloads=https://github.com/solana-labs/solana/releases/latest/download
  ```

2. Specify the download file based on your machine

  **MacOS**
  ```bash
  solana_release=solana-release-x86_64-apple-darwin.tar.bz2
  ```

  **Linux**
  ```bash
  solana_release=solana-release-x86_64-unknown-linux-gnu.tar.bz2
  ```

3. Download

  ```bash
  curl -L -sSf -o solana-release.tar.bz2 $solana_downloads/$solana_release
  ```

## Extract
Next, extract the tarball
```bash
tar xf solana-release.tar.bz2
```

## Add to "PATH"
Now add the tool to your PATH environment variable with the following command
```bash
export PATH="$(pwd)/solana-release/bin:${PATH}"
```

## Check
Finally, check that `solana-keygen` can be run by running
```bash
solana-keygen -V
```
