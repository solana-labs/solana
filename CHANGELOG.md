# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).
This project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html)
and follows a [Backwards Compatability Policy]
(https://docs.solana.com/developing/backwards-compatibility)

## Unreleased
### [1.17.0]
* Changes
  * Added a changelog!
* Upgrade Notes

## Released
### 2023-05-31 - [1.16.0](https://github.com/solana-labs/solana/releases/tag/v1.16.0)

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
    * dapp developers to make changes.
* Link to any relevant feature gate issues or SIMDs.
* If you add entries on multiple branches use the same wording if possible.
This simplifies the process of diffing between versions of the log.

## Maintaining This Changelog
* When a release is tagged and the version is bumped:
  * Move that release's notes to the Released section.
  * Add a new section to Unreleased for the next anticipated release.
