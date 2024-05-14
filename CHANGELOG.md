# Changelog

All notable changes to this project will be documented in this file.

Please follow the [guidance](#adding-to-this-changelog) at the bottom of this file when making changes
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).
This project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html)
and follows a [Backwards Compatibility Policy](https://docs.solanalabs.com/backwards-compatibility)

Release channels have their own copy of this changelog:
* [edge - v2.0](#edge-channel)
* [beta - v1.18](https://github.com/solana-labs/solana/blob/v1.18/CHANGELOG.md)
* [stable - v1.17](https://github.com/solana-labs/solana/blob/v1.17/CHANGELOG.md)

<a name="edge-channel"></a>
## [2.0.0] - Unreleased
* Changes
  * `central-scheduler` as default option for `--block-production-method` (#34891)
  * `solana-rpc-client-api`: `RpcFilterError` depends on `base64` version 0.22, so users may need to upgrade to `base64` version 0.22

## [1.18.0]
* Changes
  * Added a github check to support `changelog` label
  * The default for `--use-snapshot-archives-at-startup` is now `when-newest` (#33883)
    * The default for `solana-ledger-tool`, however, remains `always` (#34228)
  * Added `central-scheduler` option for `--block-production-method` (#33890)
  * Updated to Borsh v1
  * Added allow_commission_decrease_at_any_time feature which will allow commission on a vote account to be
    decreased even in the second half of epochs when the commission_updates_only_allowed_in_first_half_of_epoch
    feature would have prevented it
  * Updated local ledger storage so that the RPC endpoint
    `getSignaturesForAddress` always returns signatures in block-inclusion order
  * RPC's `simulateTransaction` now returns `innerInstructions` as `json`/`jsonParsed` (#34313).
  * Bigtable upload now includes entry summary data for each slot, stored in a
    new `entries` table
  * Forbid multiple values for the `--signer` CLI flag, forcing users to specify multiple occurrences of `--signer`, one for each signature
  * New program deployments default to the exact size of a program, instead of
    double the size. Program accounts must be extended with `solana program extend`
    before an upgrade if they need to accommodate larger programs.
  * Interface for `gossip_service::get_client()` has changed. `gossip_service::get_multi_client()` has been removed.
* Upgrade Notes
  * `solana-program` and `solana-sdk` default to support for Borsh v1, with
limited backward compatibility for v0.10 and v0.9. Please upgrade to Borsh v1.
  * Operators running their own bigtable instances need to create the `entries`
    table before upgrading their warehouse nodes

## [1.17.0]
* Changes
  * Added a changelog.
  * Added `--use-snapshot-archives-at-startup` for faster validator restarts
* Upgrade Notes

## Adding to this Changelog
### Audience
* Entries in this log are intended to be easily understood by contributors,
consensus validator operators, rpc operators, and dapp developers.

### Noteworthy
* A change is noteworthy if it:
  * Adds a feature gate, or
  * Implements a SIMD, or
  * Modifies a public API, or
  * Changes normal validator / rpc run configurations, or
  * Changes command line arguments, or
  * Fixes a bug that has received public attention, or
  * Significantly improves performance, or
  * Is authored by an external contributor.

### Instructions
* Update this log in the same pull request that implements the change. If the
change is spread over several pull requests update this log in the one that
makes the feature code complete.
* Add notes to the [Unreleased] section in each branch that you merge to.
  * Add a description of your change to the Changes section.
  * Add Upgrade Notes if the change is likely to require:
    * validator or rpc operators to update their configs, or
    * dapp or client developers to make changes.
* Link to any relevant feature gate issues or SIMDs.
* If you add entries on multiple branches use the same wording if possible.
This simplifies the process of diffing between versions of the log.

## Maintaining This Changelog
### When creating a new release branch:
* Commit to master updating the changelog:
  * Update the edge, beta, and stable links
  * Create new section: `vx.y+1.0 - Unreleased`
  * Remove `Unreleased` annotation from vx.y.0 section.
* Create vx.y branch starting at that commit
* Tag that commit as vx.y.0

### When creating a new patch release:
* Commit to the release branch updating the changelog:
  * Remove `Unreleased` annotation from `vx.y.z` section
  * Add a new section at the top for `vx.y.z+1 - Unreleased`
* Tag that new commit as the new release
