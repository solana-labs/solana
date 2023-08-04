---
title: Backwards Compatibility Policy
---

As the Solana developer ecosystem grows, so does the need for clear expectations around
breaking API and behavior changes affecting applications and tooling built for Solana.
In a perfect world, Solana development could continue at a very fast pace without ever
causing issues for existing developers. However, some compromises will need to be made
and so this document attempts to clarify and codify the process for new releases.

### Expectations

- Solana software releases include APIs, SDKs, and CLI tooling (with a few [exceptions](#exceptions)).
- Solana software releases *do not always* follow semantic versioning, more details below.
- Software for a `MINOR` version release will be compatible with the previous
  `MINOR` releases, and following `MINOR` releases, for at least one year.

### Deprecation Process

1. In any `PATCH` or `MINOR` release, a feature, API, endpoint, etc. could be marked as deprecated.
2. According to code upgrade difficulty, some features will remain deprecated for a few release
   cycles.
3. At least one year later, deprecated features may be removed in an incompatible way.

### Release Cadence

The Solana RPC API, Rust SDK, CLI tooling, and SBF Program SDK are all updated and shipped
along with each Solana software release and should always be compatible between `PATCH`
updates of a particular `MINOR` version release.

#### Release Channels

- `edge` software that contains cutting-edge features with no backwards compatibility policy
- `beta` software that runs on the Solana Testnet cluster
- `stable` software that run on the Solana Mainnet Beta and Devnet clusters

#### Major Releases (x.0.0)

RPC `MAJOR` version releases (e.g. 2.0.0) may contain breaking changes and removal
of previously deprecated features.

We do not expect the Rust SDK `MAJOR` version to be changed. Any breaking changes
will be done though the deprecation process described in
[Why not just use SemVer](#why-not-just-use-semver) in a `MINOR` version.

#### Minor Releases (1.x.0)

New features and proposal implementations are added to _new_ `MINOR` version
releases (e.g. 1.4.0) and are first run on Solana's Testnet cluster. While running
on the testnet, `MINOR` versions are considered to be in the `beta` release channel. After
those changes have been patched as needed and proven to be reliable, the `MINOR` version will
be upgraded to the `stable` release channel and deployed to the Mainnet Beta cluster.

The Rust SDK may contain breaking changes and removal of previously deprecated features
in a `MINOR` release. Every `MINOR` release will be compatible with at least one year of
preceding `MINOR` releases, and at least one year of future `MINOR` releases.

More details in [Why not just use SemVer](#why-not-just-use-semver).

#### Patch Releases (1.0.x)

Low risk features, non-breaking changes, and security and bug fixes are shipped as part
of `PATCH` version releases (e.g. 1.0.11). Patches may be applied to both `beta` and `stable`
release channels.

### RPC API

Patch releases:

- Bug fixes
- Security fixes
- Endpoint / feature deprecation

Minor releases:

- New RPC endpoints and features

Major releases:

- Removal of deprecated features

### Rust Crates

- [`solana-sdk`](https://docs.rs/solana-sdk/) - Rust SDK for creating transactions and parsing account state
- [`solana-program`](https://docs.rs/solana-program/) - Rust SDK for writing programs
- [`solana-client`](https://docs.rs/solana-client/) - Rust client for connecting to RPC API
- [`solana-cli-config`](https://docs.rs/solana-cli-config/) - Rust client for managing Solana CLI config files
- [`solana-geyser-plugin-interface`](https://docs.rs/solana-geyser-plugin-interface/) - Rust interface for developing Solana Geyser plugins.

Patch releases:

- Bug fixes
- Security fixes
- Performance improvements

Minor releases:

- New APIs
- Removal of deprecated APIs, subject to deprecation delay
- Backwards incompatible behavior changes, subject to deprecation delay

Major releases:

- Are not projected to happen ever. More details in [Why not just use SemVer](#why-not-just-use-semver).

### CLI Tools

Patch releases:

- Bug and security fixes
- Performance improvements
- Subcommand / argument deprecation

Minor releases:

- New subcommands

Major releases:

- Switch to new RPC API endpoints / configuration introduced in the previous major version.
- Removal of deprecated features

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

Breaking changes will be limited to `MAJOR` version updates. `MINOR` and `PATCH` updates should always
be backwards compatible.

### Exceptions

#### Web3 JavaScript SDK

The Web3.JS SDK also follows semantic versioning specifications but is shipped separately from Solana
software releases.

#### Attack Vectors

If a new attack vector is discovered in existing code, the above processes may be
circumvented in order to rapidly deploy a fix, depending on the severity of the issue.

#### CLI Tooling Output

CLI tooling json output (`output --json`) compatibility will be preserved; however, output directed
for a human reader is subject to change. This includes output as well as potential help, warning, or
error messages.

### Why not just use SemVer

The Solana Rust SDK crates do *not* follow semantic versioning (SemVer) for
breaking changes.

#### SemVer in Rust

Under SemVer, breaking changes, such as removal of functions or types, only happens
in a new major version.

In many situations, this is useful -- it is undesirable to pick up a breaking
change accidentally. The default dependency declaration in Cargo follows SemVer.
It assumes that all minor versions are compatible, and will automatically update
to the newest minor version available.

For example, if you declare `solana-program = "1.10"`, Cargo can pull in version
`1.16`, since it assumes that all `1.X.Y` releases are compatible.

If, however, Cargo deems that two versions of a package are incompatible, it will
treat them as two completely separate packages.

Packages can be deemed incompatible for many reasons, but the most common situation
under the default declaration format (SemVer) is due to different major versions.

If a package ends up with two or more instances of `solana-program`,
[bad things will happen](https://doc.rust-lang.org/cargo/reference/resolver.html#version-incompatibility-hazards),
either at compile-time or runtime, if the conflicting types in the public APIs
are ever used together.

Because of these issues, developers should only want one instance of `solana-program`
in their project. This includes *all* direct and indirect dependencies: they should
all use the same `solana-program`.

If developers all use the default versioning declaration with major version `1`,
then we can ensure that only one version of `solana-program` exists in the resolution.

The specifics of the Cargo resolver are beyond the scope of this document.
You can find more information about the Cargo resolver in
[The Cargo Book](https://doc.rust-lang.org/cargo/reference/resolver.html).

#### So what's your solution?

We would like all releases to be 100% backwards compatible, allowing everyone
to upgrade at any time with no effort.  This model, however, would put overly
strict constraints on the development of the Rust SDK crates, considering we cannot
in practice increase the major version.

So we settle on a compromise.  We guarantee full compatibility, but only for a
certain period of time.  First we deprecate APIs, and only remove or break the
API after at least one year has passed.

We are in the process of setting up tests that verify compatibility guarantees to match
our policy.  But, as any test, it may have gaps, and is subject to engineering time
invested in the test coverage.

Should we break backwards compatibility, we will do whatever is necessary to restore
functionality, within reason.

This model has precedents in many other platforms.
[Rust editions](https://blog.rust-lang.org/2021/05/11/edition-2021.html#what-is-an-edition),
can introduce backwards-incompatible changes.

And there are even more examples in
[Why Semantic Versioning Isn't](https://gist.github.com/jashkenas/cbd2b088e20279ae2c8e):

> Node doesn't follow SemVer, Rails doesn't do it, Python doesn't do it, Ruby doesn't do it, even npm doesn't follow SemVer.

The Solana SDK is closer to a platform than a library, so it makes more sense to
follow other platforms.

Additionally, the
[Minimum Supported Rust Version (MSRV) Policies](https://github.com/rust-lang/api-guidelines/discussions/231)
suggest that updating MSRV is *not* a SemVer breaking change, even though it
effectively forces users to upgrade their compiler just to update a crate.

By ensuring that projects will never immediately break, and giving developers
one year to update, this model will keep stability while avoiding stagnation.
