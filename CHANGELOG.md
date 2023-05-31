# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).
This project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html) and follows a [Backwards Compatability Policy](https://docs.solana.com/developing/backwards-compatibility)

## Adding to this Changelog
* Entries in this log are intended to be easily understood by consensus validators, rpc operators, and dapp developers.
* Add notes to the [Unreleased] section that corresponds with the branch you're updating.
    * Add a description of your change to the Changes section
    * If your change is likely to require operators to update their configs add to the Upgrade Notes
        * If you backport your change to the previous release branch or you're updating the stable branch, add note to the "patch" section
        * If you don't backport, add node to the "minor version" section (these notes will be carried forward for releases on this branch until the previous branch is no longer supported)

## How To Maintain This Changelog
* When the version is bumped:
    * Patch
        * Date and tag the [Unreleased] section with the new release including a link to the release page
        * Create new [Unreleased] section and copy the minor upgrade notes into it
    * Minor
        * Advance the version numbers in the [Unreleased sections]

## Unreleased
### [master - edge]
* Changes
* Upgrade Notes
    * Minor version upgrade
-

### [v1.16 - beta]
* Changes
  * Added a change log!
* Upgrade Notes
  * Patch upgrade
  * Minor version upgrade

### [v1.14 - stable]
* Changes
* Upgrade Notes
  * Patch upgrade

## Released
### 2022-07-23 - [1.11.4] 

### 2022-07-26 - [1.10.33]
- Fix bug to enable support for token-2022 accounts in preTokenBalances/postTokenBalances
- Add Display implementations for various zk-token-sdk Pod structs
