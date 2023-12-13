# Changelog
All notable changes to this project will be documented in this file.

Please follow the [guidance](#adding-to-this-changelog) at the bottom of this file when making changes
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).
This project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html)
and follows a [Backwards Compatibility Policy](https://docs.solana.com/developing/backwards-compatibility)

## [1.17.3] - Unreleased
* Changes
* Upgrade Notes

## [1.17.2]
* minor bug fixes and improvements

## [1.17.1]
* minor bug fixes and improvements

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
  * Remove `Unreleased` annotation from vx.y.0 section.
  * Create new section: `vx.y+1.0 - Unreleased`
* Create vx.y branch starting at that commit
* Tag that commit as vx.y.0

### When creating a new patch release:
* Commit to the release branch updating the changelog:
  * Remove `Unreleased` annotation from `vx.y.z` section
  * Add a new section at the top for `vx.y.z+1 - Unreleased`
* Tag that new commit as the new release
