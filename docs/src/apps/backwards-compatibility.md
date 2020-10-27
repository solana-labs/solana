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
- Software for a `MINOR` version release will be compatible across all software back to the
  the first `PATCH` release of the previous `MINOR` version. (eg. v1.4.x will remain
  compatible back to v1.3.0)
- Solana **DOES NOT** guarantee that software on _non-sequential_ `MINOR` version releases
  will always be compatible. _(e.g. 1.3.x will likely not be compatible with 1.5.x)_
- Solana software releases **DO NOT** strictly follow semantic versioning, details below.

### Deprecation Process

1. In any `PATCH` release (e.g. 1.3.x), a feature, API, endpoint, etc. will be marked as deprecated
2. The next `MINOR` release (e.g. 1.4.0) will also contain the deprecation without breaking compatibility
3. Finally, the following `MINOR` release (e.g. 1.5.0) may remove the deprecated feature
  in an incompatible way.

### Release Cadence

The Solana RPC API, Rust SDK, CLI tooling, and BPF Program SDK are all updated and shipped
along with each Solana software release and should always be compatible between `PATCH`
updates of a particular `MINOR` version release.

#### Minor Releases (1.x.0)

Experimental changes and new proposal implementations are added to _new_ `MINOR` version
releases (e.g. 1.4.0) and are first run on Solana's Tour de SOL testnet cluster. After
those changes have proven to be stable, the Mainnet Beta cluster will be updated to the
new `MINOR` version.

#### Patch Releases (1.0.x)

Low risk features, non-breaking changes, and security and bug fixes are shipped as part
of `PATCH` version releases (e.g. 1.0.11).

### RPC API

Patch releases:
- New RPC endpoints and features
- Bug fixes
- Security fixes

Minor releases:
- Endpoint / feature deprecation
- Removal of previous `MINOR` version deprecated features

### Rust Crates

* [`solana-sdk`](https://docs.rs/solana-sdk/) - Rust SDK for creating transactions and parsing account state
* [`solana-program`](https://docs.rs/solana-program/) - Rust SDK for writing programs
* [`solana-client`](https://docs.rs/solana-client/) - Rust client for connecting to RPC API
* [`solana-cli-config`](https://docs.rs/solana-cli-config/) - Rust client for managing Solana CLI config files

Patch releases:
- New APIs
- Bug fixes
- Performance improvements
- Security fixes

Minor releases:
- Removal of deprecated APIs
- Backwards incompatible behavior changes

### CLI Tools

Patch releases:
- New subcommands
- Bug and security fixes
- Performance improvements
- Subcommand / argument deprecation

Minor releases:
- Switch to new RPC API endpoints / configuration introduced in the previous minor version.
- Removal of previous minor version deprecated features

### Runtime Features

New Solana runtime features are feature-switched and manually activated. Runtime features
include: the introduction of new native programs, sysvars, and syscalls; and changes to
their behavior. Feature activation is cluster agnostic, allowing confidence to be built on
Testnet before activation on Mainnet-beta.

The release process is as follows:

1. New runtime feature is included in a new release, deactivated by default
2. Once sufficient staked validators upgrade to the new release, the runtime feature switch
  is activated manually with an instruction
3. The feature takes effect at the beginning of the next epoch

### Infrastructure Changes

#### Public API Nodes

Solana provides publicly available RPC API nodes for all developers to use. The Solana team
will make their best effort to communicate any changes to the host, port, rate-limiting behavior,
availability, etc. However, we recommend that developers rely on their own validator nodes to
discourage dependence upon Solana operated nodes.

#### Local cluster scripts and Docker images

Breaking changes will be limited to `MINOR` version updates. `PATCH` updates should always
be backwards compatible.

### Exceptions

#### Web3 JavaScript SDK

The Web3.JS SDK follows semantic versioning specifications and is shipped separately from Solana
software releases.

#### Attack Vectors

If a new attack vector is discovered in existing code, the above processes may be
circumvented in order to rapidly deploy a fix, depending on the severity of the issue.
