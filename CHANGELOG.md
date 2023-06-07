# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).
This project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html)
and follows a [Backwards Compatability Policy]
(https://docs.solana.com/developing/backwards-compatibility)

## Adding to this Changelog
### Audience
* Entries in this log are intended to be easily understood by consensus
validators, rpc operators, and dapp developers.

### Noteworthy
* Changes that relate to a SIMD, or add a feature gate should add an entry to
this log. Other changes that are relevant to the aforementioned audiences should
add an entry at the discretion of authors and reviewers.

### Instructions
* Update this log in the same pull request that implements the change. If the
change is spread over several pull requests update this log in the one that
makes the feature code complete.
* Add notes to the [Unreleased] section in each branch that you merge to.
  * Add a description of your change to the Changes section
  * If your change is likely to require operators to update their configs add to
  the Upgrade Notes
* If you add entries on multiple branches please use the same wording if
possible. This simplifies the process of diffing between versions of the log.

## How To Maintain This Changelog
* When a release is tagged and the version is bumped:
  * Move that release's notes to the Released section
  * Add a new section to Unreleased for the next anticipated release.

## Unreleased
* Changes
  * Added a changelog!
* Upgrade Notes

## Released
### 2023-05-31 - [1.16.0](https://github.com/solana-labs/solana/releases/tag/v1.16.0)
