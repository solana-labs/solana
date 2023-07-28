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
- Software for a `MINOR` version release will be compatible with the previous 3
  `MINOR` releases, and following 3 `MINOR` releases, accounting for skipped minor
  versions, more details below.

### Deprecation Process

1. In any `PATCH` or `MINOR` release, a feature, API, endpoint, etc. could be marked as deprecated.
2. According to code upgrade difficulty, some features will remain deprecated for a few release
   cycles.
3. At least 4 `MINOR` releases later, deprecated features may be removed in an incompatible way.

### Release Cadence

The Solana RPC API, Rust SDK, CLI tooling, and SBF Program SDK are all updated and shipped
along with each Solana software release and should always be compatible between `PATCH`
updates of a particular `MINOR` version release.

#### Release Channels

- `edge` software that contains cutting-edge features with no backwards compatibility policy
- `beta` software that runs on the Solana Testnet cluster
- `stable` software that run on the Solana Mainnet Beta and Devnet clusters

#### Major Releases (x.0.0)

`MAJOR` version releases (e.g. 2.0.0) may contain breaking changes and removal of previously
deprecated features. Client SDKs and tooling will begin using new features and endpoints
that were enabled in the previous `MAJOR` version.

The Rust SDK will not have any future `MAJOR` releases, and will stay on `1.X` to
provide a better developer experience. More details in
[Why not just use semver](#why-not-just-use-semver).

#### Minor Releases (1.x.0)

New features and proposal implementations are added to _new_ `MINOR` version
releases (e.g. 1.4.0) and are first run on Solana's Testnet cluster. While running
on the testnet, `MINOR` versions are considered to be in the `beta` release channel. After
those changes have been patched as needed and proven to be reliable, the `MINOR` version will
be upgraded to the `stable` release channel and deployed to the Mainnet Beta cluster.

The Rust SDK may contain breaking changes and removal of previously deprecated features
in a `MINOR` release. According to code upgrade difficulty, every `MINOR` release
will be compatible with at least the 3 preceding `MINOR` releases and 3 future
`MINOR` releases.

For example, software using version `1.20` will be compatible with versions up
to `1.23` going forward, and back to `1.17`. Any release numbers that are skipped,
such as `1.15`, are not considered in this calculation.

At a projected cadence of 4 minor releases per year, that gives developers at least
one year to update a project.

More details in [Why not just use semver](#why-not-just-use-semver).

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
- Removal of deprecated APIs
- Backwards incompatible behavior changes

Major releases:

- Are not projected to happen ever. More details in [Why not just use semver](#why-not-just-use-semver).

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

### Why not just use semver

The Solana Rust SDK crates use a model for introducing breaking changes that does
*not* follow semantic versioning (semver).

#### Semver in Rust

Under semver, breaking changes, such as removal of functions or types, only happens
in a new major version.

In many situations, this is useful -- it's very difficult to pick up a breaking
change accidentally. The default dependency declaration in Cargo follows this
concept. It assumes that all minor versions are compatible, and will automatically
update to the newest minor version available.

For example, if you declare `solana-program = "1.10"`, Cargo can pull in version
`1.16`, since it assumes that all `1.X.Y` releases are compatible.

Additionally, Cargo will force all crates in a build to use the same major version
of a dependency. For example, if your crate `my-crate` declares `solana-program = "1.10"`,
and a dependency `my-dependency` declares `solana-program = "1.16"`, both your crate and your
dependency will be built with version `1.16`, at least.

Additionally, the types in `solana-program` used by both your crate and your
dependency are treated as the same.

For example, if `solana-program` has a struct declared as:

```
#[derive(PartialEq)]
pub struct Pubkey(pub [u8; 32]);
```

And `my-dependency` has:

```
pub const DEPENDENCY_PUBKEY = Pubkey([1; 32]);
```

Then `my-crate` can say:

```
use solana_program::Pubkey;
let my_pubkey = Pubkey([1; 32]);
if my_pubkey == my_dependency::DEPENDENCY_PUBKEY {
    println!("They're the same");
}
```

On the other hand, types from crates with different major versions are treated as
completely different types. If `my-crate` declares `solana-program = "2"` and
`my-dependency` declares `solana-program = "1"`, then the preceding code *will not*
compile.

To get this code to compile, you need to create functions to convert between the
types in v1 and v2. And whenever v3 is released, the entire problem becomes multiplied.

#### So what's your solution?

Rather than forcing developers to disentangle these kinds of messes on every
major version release, our solution is to keep everything on the same major version.

However, to avoid chaos and ensure that no downstream client ever breaks from updating
from one `MINOR` version to the next, we must provide new functions or structs
and deprecate the old ones.

We have broken this commitment in the past, but through expanded downstream
testing and conscientiously exposing APIs, we aim to never repeat that mistake.
If it does happen accidentally, we will do whatever is necessary to restore
the functionality, within reason.

To avoid maintaining too much old code, however, we reserve the right to remove
deprecated code after at least one year, or roughly 4 `MINOR` versions later.

This model has precedents in the Rust ecosystem. Through
[Rust editions](https://blog.rust-lang.org/2021/05/11/edition-2021.html#what-is-an-edition),
the Rust developers can introduce backwards-incompatible changes, all while staying
on major version 1. As crate developers, we have less flexibility than a language,
but the concept is similar.

Additionally, the
[Minimum Supported Rust Version (MSRV) Policies](https://github.com/rust-lang/api-guidelines/discussions/231)
suggest that updating MSRV is *not* a semver breaking change, even though it
effectively forces users to upgrade their compiler just to update a crate.

By ensuring that projects will never immediately break, and giving developers
one year to update, this model will keep some stability while avoiding stagnation.

#### But why was the upgrade to 1.16 so difficult?

In version `1.16` of the Rust crates, the Borsh dependency was upgraded from
`0.9` to `0.10`, which constituted a breaking change.

Downstream developers faced unsolvable resolution problems because their dependencies
were also forced to use `1.16`, but if the dependencies were not explicitly updated
to use Borsh `0.10`, they were met with incompatible traits, since they implemented
`BorshSerialize` and `BorshDerialize` on version 0.9, which as described earlier,
are treated as completely different traits from `BorshSerialize` and `BorshDeserialize`
on version 0.10.

To address this problem, we've implemented the older Borsh 0.9 traits on required
types, and exposed the Solana Borsh utilities through explicit versioning. This
allows us to upgrade to any other version of Borsh without ever breaking a
downstream developer.

In a year, however, we reserve the right to remove the implementations of the
Borsh 0.9 traits and potentially break downstream projects that have not updated
within the last year.

Even if the Rust crates implemented proper semantic versioning, downstream
projects would have been broken as soon as they tried to mix `Pubkey`s from
a non-updated dependency with `Pubkey`s from the new major version.
