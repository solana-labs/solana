# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
See [Adding to this Changelog](#adding-to-this-changelog) section below for details on how to add notes to this file

## [Unreleased master - edge]
### Changes
-
### Upgrade Notes
#### Minor version upgrade
-

## [Unreleased v1.11 - beta]
### Changes
- Added a change log!
### Upgrade Notes
#### Patch upgrade
-
#### Minor version upgrade
-

## [Unreleased v1.10 - stable]
### Changes
-
### Upgrade Notes
#### Patch upgrade
-

## [1.11.4] - 2022-07-23

## [1.10.33] - 2022-07-26
- Fix bug to enable support for token-2022 accounts in preTokenBalances/postTokenBalances
- Add Display implementations for various zk-token-sdk Pod structs

## Adding to this Changelog
* Entries in this log are intended to be easily understood by users.
* Add notes to the [Unreleased] section that corresponds with the branch you're updating.
    * Add a description of your change to the Changes section
    * If your change is likely to require validators to update their configs add to the Upgrade Notes
        * If you backport your change to the previous release branch or you're updating the stable branch, add note to the "patch" section
        * If you don't backport, add node to the "minor version" section (these notes will be carried forward for releases on this branch until the previous branch is no longer supported)

## How To Maintain This Changelog
* When the version is bumped:
    * Patch
        * Tag and date the [Unreleased] section with the new release including a link to the release page
        * Create new [Unreleased] section and copy the minor upgrade notes into it
    * Minor
        * Advance the version numbers in the [Unreleased sections]
