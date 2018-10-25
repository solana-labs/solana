# Solana Release process

## Introduction

Solana uses a channel-oriented, date-based branching process described [here](https://github.com/solana-labs/solana/blob/master/rfcs/rfc-005-branches-tags-and-channels.md).

## Release Steps

### Changing channels

When cutting a new channel branch these pre-steps are required:

1. Pick your branch point for release on master.
2. Create the branch.  The name should be "v" + the first 2 "version" fields from Cargo.toml.  For example, a Cargo.toml with version = "0.9.0" implies the next branch name is "v0.9".
3. Update Cargo.toml to the next semantic version (e.g. 0.9.0 -> 0.10.0) by running `./scripts/increment-cargo-version.sh`.
4. Push your new branch to solana.git
5. Land your Cargo.toml change as a master PR.

At this point, ci/channel-info.sh should show your freshly cut release branch as "BETA_CHANNEL" and the previous release branch as "STABLE_CHANNEL".

### Updating channels (i.e. "making a release")

We use [github's Releases UI](https://github.com/solana-labs/solana/releases) for tagging a release.

1. Go [there ;)](https://github.com/solana-labs/solana/releases).
2. Click "Draft new release".
3. If the first major release on the branch (e.g. v0.8.0), paste in [this template](https://raw.githubusercontent.com/solana-labs/solana/master/.github/RELEASE_TEMPLATE.md) and fill it in.
4. Test the release by generating a tag using semver's rules.  First try at a release should be <branchname>.X-rc.0.
5. Verify release automation:
   1. [Crates.io](https://crates.io/crates/solana) should have an updated Solana version.
   2. ...
6. After testnet deployment, verify that testnets are running correct software.  http://metrics.solana.com should show testnet running on a hash from your newly created branch.
