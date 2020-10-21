---
title: API Breaking Changes
---

As the Solana developer ecosystem grows, so does the need for clear expectations around
breaking API and behavior changes affecting applications and tooling built for Solana.
In a perfect world, Solana development could continue at a very fast pace without ever
causing issues for existing developers. However, some compromises will need to be made
and so this document attempts to clarify and codify the process for new releases.

Solana API's, SDK's, and CLI tooling are typically tied to validator software releases.
Validator software releases DO NOT strictly follow semantic versioning.

### Release Cadence

The Solana RPC API, Rust SDK, CLI tooling, and BPF Program SDK are all updated and shipped
along with each Solana validator release and should always be compatible between patch
updates of a particular minor version release.

#### Minor Releases (1.x.0)

Experimental changes and new proposal implementations are added to new MINOR version
releases (ex. 1.4.0) and are first run on Solana's Tour de SOL testnet cluster. After
those changes have proven to be stable, the Mainnet Beta cluster will be updated to the
new version.

#### Patch Releases (1.0.x)

Low risk features, non-breaking changes, and security and bug fixes are shipped as part
of PATCH version releases (ex. 1.0.11).

### RPC API

Patch releases:
- New RPC endpoints and features
- Bug fixes
- Security fixes

Minor releases:
- Endpoint / feature deprecation
- Removal of previous minor version deprecated features

### CLI Tools

Patch releases:
- New subcommands
- Bug and security fixes
- Performance improvements

Minor releases:
- Switch to new RPC API endpoints / configuration introduced in the previous minor version.
- Subcommand / argument deprecation
- Removal of previous minor version deprecated features

### Runtime Features

New Solana runtime features are feature-switched and manually activated. Runtime features
include things like introducing new native programs, sysvars, and syscalls or changes to
their behavior.

The release process is as follows:

1. New runtime feature is added to a new release but deactivated by default
2. Sufficient staked validators upgrade to the new release
3. Runtime feature switch is activated manually with an instruction and take effect in the next slot

### Infrastructure Changes

#### Public API Nodes

Solana provides publicly available RPC API nodes for all developers to use. The Solana team
will make their best effort to communicate any changes to the host, port, rate-limiting behavior,
availability, etc. However, we recommend that developers rely on their own validator nodes to
discourage centralized usage of Solana operated nodes.

#### Local cluster scripts and Docker images

Breaking changes will be limited to minor version updates, patch updates should always
be backwards compatible.

### Exceptions

#### Web3 JavaScript SDK

The Web3.JS SDK follows semantic versioning rules and is shipped separately from validator
releases.

#### Attack Vectors

If a new attack vector is discovered in existing code and reported to the Solana team,
according to the severity of the issue, the above processes may be circumvented.
