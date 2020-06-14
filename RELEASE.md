# Solana Release process

## Branches and Tags

```
========================= master branch (edge channel) =======================>
         \                      \                     \
          \___v0.7.0 tag         \                     \
           \                      \         v0.9.0 tag__\
            \          v0.8.0 tag__\                     \
 v0.7.1 tag__\                      \                 v0.9 branch (beta channel)
              \___v0.7.2 tag         \___v0.8.1 tag
               \                      \
                \                      \
           v0.7 branch         v0.8 branch (stable channel)

```

### master branch
All new development occurs on the `master` branch.

Bug fixes that affect a `vX.Y` branch are first made on `master`.  This is to
allow a fix some soak time on `master` before it is applied to one or more
stabilization branches.

Merging to `master` first also helps ensure that fixes applied to one release
are present for future releases.  (Sometimes the joy of landing a critical
release blocker in a branch causes you to forget to propagate back to
`master`!)"

Once the bug fix lands on `master` it is cherry-picked into the `vX.Y` branch
and potentially the `vX.Y-1` branch.  The exception to this rule is when a bug
fix for `vX.Y` doesn't apply to `master` or `vX.Y-1`.

Immediately after a new stabilization branch is forged, the `Cargo.toml` minor
version (*Y*) in the `master` branch is incremented by the release engineer.
Incrementing the major version of the `master` branch is outside the scope of
this document.

### v*X.Y* stabilization branches
These are stabilization branches for a given milestone.  They are created off
the `master` branch as late as possible prior to the milestone release.

### v*X.Y.Z* release tag
The release tags are created as desired by the owner of the given stabilization
branch, and cause that *X.Y.Z* release to be shipped to https://crates.io

Immediately after a new v*X.Y.Z* branch tag has been created, the `Cargo.toml`
patch version number (*Z*) of the stabilization branch is incremented by the
release engineer.

## Channels
Channels are used by end-users (humans and bots) to consume the branches
described in the previous section, so they may automatically update to the most
recent version matching their desired stability.

There are three release channels that map to branches as follows:
* edge - tracks the `master` branch, least stable.
* beta - tracks the largest (and latest) `vX.Y` stabilization branch, more stable.
* stable - tracks the second largest `vX.Y` stabilization branch, most stable.

## Steps to Create a Branch

### Create the new branch
1. Check out the latest commit on `master` branch:
    ```
    git fetch --all
    git checkout upstream/master
    ```
1. Determine the new branch name.  The name should be "v" + the first 2 version fields
   from Cargo.toml.  For example, a Cargo.toml with version = "0.9.0" implies
   the next branch name is "v0.9".
1. Create the new branch and push this branch to the `solana` repository:
    ```
    git checkout -b <branchname>
    git push -u origin <branchname>
    ```

### Update master branch with the next version

1. After the new branch has been created and pushed, update the Cargo.toml files on **master** to the next semantic version (e.g. 0.9.0 -> 0.10.0) with:
     ```
     scripts/increment-cargo-version.sh minor
     ```
1. Rebuild to get an updated version of `Cargo.lock`:
    ```
    cargo build
    ```
1. Push all the changed Cargo.toml and Cargo.lock files to the `master` branch with something like:
    ```
    git co -b version_update
    git ls-files -m | xargs git add
    git commit -m 'Update Cargo.toml versions from X.Y to X.Y+1'
    git push -u origin version_update
    ```
1. Confirm that your freshly cut release branch is shown as `BETA_CHANNEL` and the previous release branch as `STABLE_CHANNEL`:
    ```
    ci/channel_info.sh
    ```

## Steps to Create a Release

### Create the Release Tag on GitHub

1. Go to [GitHub Releases](https://github.com/solana-labs/solana/releases) for tagging a release.
1. Click "Draft new release".  The release tag must exactly match the `version`
   field in `/Cargo.toml` prefixed by `v`.
   1.  If the Cargo.toml verion field is **0.12.3**, then the release tag must be **v0.12.3**
1. Make sure the Target Branch field matches the branch you want to make a release on.
   1.  If you want to release v0.12.0, the target branch must be v0.12
1. If this is the first release on the branch (e.g. v0.13.**0**), paste in [this
   template](https://raw.githubusercontent.com/solana-labs/solana/master/.github/RELEASE_TEMPLATE.md).  Engineering Lead can provide summary contents for release notes if needed.  If this is a patch release, review all the commits since the previous release on this branch and add details as needed.
1. Click "Save Draft", then confirm the release notes look good and the tag name and branch are correct.
1. Ensure the release is marked **"This is a pre-release"**.  This flag will then need to be be removed once the the Linux binary artifacts appear later.
1. Go back into edit the release and click "Publish release" when ready.


### Update release branch with the next patch version

1. After the new release has been tagged, update the Cargo.toml files on **release branch** to the next semantic version (e.g. 0.9.0 -> 0.9.1) with:
     ```
     $ scripts/increment-cargo-version.sh patch
     $ ./scripts/cargo-for-all-lock-files.sh update
     ```
1. Push all the changed Cargo.toml and Cargo.lock files to the **release branch** with something like:
    ```
    git co -b version_update
    git ls-files -m | xargs git add
    git commit -m 'Bump version to X.Y.Z+1'
    git push -u origin version_update
    ```

### Prepare for the next release
1.  Go to [GitHub Releases](https://github.com/solana-labs/solana/releases) and create a new draft release for `X.Y.Z+1` with empty release nodes.  This allows people to incrementally add new release notes until it's time for the next release
1.  Go to the [Github Milestones](https://github.com/solana-labs/solana/milestones).  Create a new milestone for the `X.Y.Z+1`, move over
unresolved issues still in the `X.Y.Z` milestone, then close the `X.Y.Z` milestone.

### Verify release automation success
1. Go to [Solana Releases](https://github.com/solana-labs/solana/releases) and click on the latest release that you just published.  Verify that all of the build artifacts are present.  This can take up to 60 minutes after creating the tag.
1. The `solana-secondary` Buildkite pipeline handles creating the Linux release artifacts and updated crates.  Look for a job under the tag name of the release: https://buildkite.com/solana-labs/solana-secondary.  The macOS and Windows release artifacts are produced by Travis CI: https://travis-ci.com/github/solana-labs/solana/branches
1. [Crates.io](https://crates.io/crates/solana) should have an updated Solana version.  This can take 2-3 hours

### Update documentation

After the release automation completes, to add the new release to the list of releases on docs.solana.com:
1. Go to  https://www.gitbook.com/ and click on "Go To Dashboard"
1. Find the "Solana Docs" project
1. Under "Integrations" -> "Github", "Edit configuration". Add the new release to the "Select branches to sync" input box, tclick "Next ->" and then "Go live"
1. Now find the new version in the drop down of imported releases and
   toggle "Set as main variant" if `X.Y.Z` is now the latest release on the beta
   channel.
1. Consider deleting older `X.Y.Z-1` patch releases since they are no longer relevant
1. "Save" the edits.
2. "Merge" the edits


### Update software on devnet.solana.com/testnet.solama.com/mainnet-beta.solana.com

#### devnet.solana.com
...

##### Alert the community
Notify Discord users on #validator-support that a new release for
devnet.solana.com is available

#### testnet.solana.com
...


#### mainnet-beta.solana.com
...

