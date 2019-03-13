## Cluster Software Installation and Updates
Currently users are required to build the solana cluster software themselves
from the git repository and manually update it, which is error prone and
inconvenient.

This document proposes an easy to use software install and updater that can be
used to deploy pre-built binaries for supported platforms.  Users may elect to
use binaries supplied by Solana or any other party they trust.  Deployment of
updates is managed using an on-chain update manifest program.

### Motivating Examples
#### Fetch and run a pre-built installer

With a well-known release URL, a pre-built binary can be obtained for supported
platforms:

```bash
$ curl -o solana-install https://github.com/solana-labs/solana/releases/download/v1.2.3/solana-install-x86_64-apple-darwin
$ chmod +x ./solana-install
$ ./solana-install --help
```

Note: future enhancements should include building wrapper in the style of
https://rustup.rs/ to make bootstrapping an install even easier.

#### Build and run the installer from source

If a pre-built binary is not available for a given platform, building the
installer from source is always an option:
```bash
$ git clone https://github.com/solana-labs/solana.git
$ cd solana/install
$ cargo run -- --help
```

#### Deploy a new update to a cluster
Given a solana release tarball (as created by `ci/publish-tarball.sh`) that has already been uploaded to a publicly accessible URL,
the following commands will deploy the update:
```bash
$ solana-keygen -o update-manifest.json  # <-- only generated once, the public key is shared with users
$ solana-install deploy http://example.com/path/to/solana-release.tar.bz2 update-manifest.json
```

Note: Supporting IPFS download URLs in the future would be attractive.

#### Run a validator node that auto updates itself

```bash
$ solana-install init --pubkey 92DMonmBYXwEMHJ99c9ceRSpAmk9v6i3RdvDdXaVcrfj  # <-- pubkey is obtained from whoever is deploying the updates
$ export PATH=~/.local/share/solana-install/bin:$PATH
$ solana-keygen ...  # <-- runs the latest solana-keygen
$ solana-install run solana-fullnode ...  # <-- runs a fullnode, restarting it as necesary when an update is applied
```

### Update Manifest on-chain program
The Update Manifest program is used to advertise the deployment of new release tarballs
on a solana cluster.  Each instance of this program describes a logical update
channel for a given target triple (eg, `x86_64-apple-darwin`).  The public key
of each program instance is well-known between the entity deploying new updates
and users consuming those updates.

The update tarball itself is hosted elsewhere, off-chain and can be fetched from
the specified `download_url`.

```rust,ignore
use solana_sdk::signature::Signature;

/// Information required to download and apply a given update
pub struct UpdateManifest {
  pub target: String,                 // Target triple (TARGET)
  pub commit: String,                 // git sha1 of this update, must match the commit sha1 in the release tar.bz2
  pub timestamp_secs: u64,            // When the release was deployed (seconds since UNIX EPOC)
  pub download_url: String,           // Download URL to the release tar.bz2
  pub download_signature: Signature,  // Signature of the release tar.bz2 file, verify with the Account public key
}

/// Userdata of an Update Manifest program Account.
pub struct SignedUpdateManifest {
  pub manifest: UpdateManifest,
  pub manifest_signature: Signature,  // Signature of UpdateInfo, verify with the Account public key
}
```

Note that the `manifest` field itself contains a corresponding signature
(`manifest_signature`) to guard against man-in-the-middle attacks between the
`solana-install` tool and the solana cluster RPC API.

To guard against rollback attacks, `solana-install` will refuse to install an
update with an older `timestamp_secs` than what is currently installed.

### solana-install Tool

The `solana-install` tool is used by the user to install and update their cluster software.

It manages the following files and directories in the user's home directory:
* `~/.config/solana/updater.json` - user configuration and information about currently installed software version
* `~/.local/share/solana-install/bin` - a symlink to the current release. eg, `~/.local/share/solana-update/<update-pubkey>-<manifest_signature>/bin`
* `~/.local/share/solana-install/<update-pubkey>-<manifest_signature>/` - contents of the release
* `~/.local/share/solana-install/<update-pubkey>-<manifest_signature>.tmp/` - temporary directory used while downloading a new release

#### Command-line Interface
```bash
$ solana-install --help
solana-install 0.13.0
The solana cluster software installer

USAGE:
    solana-install [SUBCOMMAND]

FLAGS:
    -h, --help    Prints help information

SUBCOMMANDS:
    deploy    deploys a new update
    help      Prints this message or the help of the given subcommand(s)
    info      displays information about the current installation
    init      initializes a new installation
    run       Runs a program while periodically checking and applying software updates. The program will be
              restarted upon a successful software update
    update    checks for an update, and if available downloads and applies it
```

##### init
```bash
$ solana-install init --help
solana-install-init
initializes a new installation

USAGE:
    solana-install init [OPTIONS]

FLAGS:
    -h, --help    Prints help information

OPTIONS:
    -u, --url <URL>          JSON RPC URL for the solana cluster [default: https://api.testnet.solana.com/]
    -p, --pubkey <PUBKEY>    Public key of the update manifest [default: Solana-managed update manifest]
```

##### info
```bash
$ solana-install info --help
solana-install-info
displays information about the current installation

USAGE:
    solana-install info [FLAGS]

FLAGS:
    -h, --help    Prints help information
    -l            only display local information, don't check the cluster for new updates
```

##### deploy
```bash
$ solana-install deploy --help
solana-install-deploy
deploys a new update

USAGE:
    solana-install deploy <URL> <PATH>

FLAGS:
    -h, --help    Prints help information

ARGS:
    <URL>     URL to the solana release archive
    <PATH>    Keypair file for the update manifest (/path/to/keypair.json)
```

##### update
```bash
$ solana-install update --help
solana-install-update
checks for an update, and if available downloads and applies it

USAGE:
    solana-install update

FLAGS:
    -h, --help    Prints help information
```

##### run
```bash
$ solana-install run --help
solana-install-run
Runs a program while periodically checking and applying software updates. The program will be restarted upon a
successful software update

USAGE:
    solana-install run <program_name> [program_arguments]...

FLAGS:
    -h, --help    Prints help information

ARGS:
    <program_name>            program to run
    <program_arguments>...    arguments to supply to the program
```

### Release Archive Contents
A release archive is expected to be a tar file compressed with
bzip2 with the following internal structure:

* `/version.yml` - a simple YAML file containing the fields (1) `"commit"` - the git
  sha1 for this release, and (2) `"target"` - the target tuple.  Any additional
  fields are ignored.
* `/bin/` -- directory containing available programs in the release.
  `solana-install` will symlink this directory to
  `~/.local/share/solana-install/bin` for use by the `PATH` environment
  variable.
* `...` -- any additional files and directories are permitted
