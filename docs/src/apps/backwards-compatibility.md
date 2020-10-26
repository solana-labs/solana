---
title: Backwards Compatibility Policy
---

As the Solana developer ecosystem grows, so does the need for clear expectations around
breaking API and behavior changes affecting applications and tooling built for Solana.
In a perfect world, Solana development could continue at a very fast pace without ever
causing issues for existing developers. However, some compromises will need to be made
and so this document attempts to clarify and codify the process for new releases.

### Expectations

- Solana APIs, SDKs, and CLI tooling are updated together in every Solana software release
  (with a few [exceptions](#exceptions)).
- Software for a `MINOR` version release will be compatible across all software on the
  the first `PATCH` release of the previous `MINOR` version.
- Solana **DOES NOT** guarantee that software on _non-sequential_ `MINOR` version releases
  will always be compatible. _(For example, 1.3.x will likely not be compatible with 1.5.x)_
- Solana software releases **DO NOT** strictly follow semantic versioning, details below.

### Release Cadence

The Solana RPC API, Rust SDK, CLI tooling, and BPF Program SDK are all updated and shipped
along with each Solana software release and should always be compatible between patch
updates of a particular minor version release.

#### Minor Releases (1.x.0)

Experimental changes and new proposal implementations are added to new `MINOR` version
releases (ex. 1.4.0) and are first run on Solana's Tour de SOL testnet cluster. After
those changes have proven to be stable, the Mainnet Beta cluster will be updated to the
new minor version.

#### Patch Releases (1.0.x)

Low risk features, non-breaking changes, and security and bug fixes are shipped as part
of `PATCH` version releases (ex. 1.0.11).

### RPC API

Patch releases:
- New RPC endpoints and features
- Bug fixes
- Security fixes

Minor releases:
- Endpoint / feature deprecation
- Removal of previous minor version deprecated features

### Program SDK

Patch releases:
- New APIs
- Bug fixes
- Performance improvements
- Security fixes

Minor releases:
- Removal of deprecated APIs
- Backward compatibility breaking changes

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
include: the introduction of new native programs, sysvars, and syscalls; and changes to
their behavior.

The release process is as follows:

1. New runtime feature is included in a new release, deactivated by default
2. Once sufficient staked validators upgrade to the new release, the runtime feature switch is activated manually with an instruction
3. The feature takes effect at the beginning of the next epoch

### Infrastructure Changes

#### Public API Nodes

Solana provides publicly available RPC API nodes for all developers to use. The Solana team
will make their best effort to communicate any changes to the host, port, rate-limiting behavior,
availability, etc. However, we recommend that developers rely on their own validator nodes to
discourage centralized usage of Solana operated nodes.

#### Local cluster scripts and Docker images

Breaking changes will be limited to minor version updates. Patch updates should always
be backwards compatible.

### Exceptions

#### Web3 JavaScript SDK

The Web3.JS SDK follows semantic versioning specifications and is shipped separately from Solana
software releases.

#### Attack Vectors

If a new attack vector is discovered in existing code, the above processes may be
circumvented in order to rapidly deploy a fix, depending on the severity of the issue.
