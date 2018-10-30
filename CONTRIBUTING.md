Solana Coding Guidelines
===

The goal of these guidelines is to improve developer productivity by allowing developers to
jump any file in the codebase and not need to adapt to inconsistencies in how the code is
written. The codebase should appear as if it had been authored by a single developer. If you
don't agree with a convention, submit a PR patching this document and let's discuss! Once
the PR is accepted, *all* code should be updated as soon as possible to reflect the new
conventions.

Rust coding conventions
---

* All Rust code is formatted using the latest version of `rustfmt`. Once installed, it will be
  updated automatically when you update the compiler with `rustup`.

* All Rust code is linted with Clippy. If you'd prefer to ignore its advice, do so explicitly:

  ```rust
  #[cfg_attr(feature = "cargo-clippy", allow(too_many_arguments))]
  ```

  Note: Clippy defaults can be overridden in the top-level file `.clippy.toml`.

* For variable names, when in doubt, spell it out. The mapping from type names to variable names
  is to lowercase the type name, putting an underscore before each capital letter. Variable names
  should *not* be abbreviated unless being used as closure arguments and the brevity improves
  readability. When a function has multiple instances of the same type, qualify each with a
  prefix and underscore (i.e. alice_keypair) or a numeric suffix (i.e. tx0).

* For function and method names, use `<verb>_<subject>`. For unit tests, that verb should
  always be `test` and for benchmarks the verb should always be `bench`. Avoid namespacing
  function names with some arbitrary word. Avoid abreviating words in function names.

* As they say, "When in Rome, do as the Romans do." A good patch should acknowledge the coding
  conventions of the code that surrounds it, even in the case where that code has not yet been
  updated to meet the conventions described here.


Terminology
---

Inventing new terms is allowed, but should only be done when the term is widely used and
understood. Avoid introducing new 3-letter terms, which can be confused with 3-letter acronyms.

Some terms we currently use regularly in the codebase:

* fullnode: n. A fully participating network node.
* hash: n. A SHA-256 Hash.
* keypair: n. A Ed25519 key-pair, containing a public and private key.
* pubkey: n. The public key of a Ed25519 key-pair.
* sigverify: v. To verify a Ed25519 digital signature.


Proposing architectural changes
---

Solana's architecture is described by markdown files in the `doc/` directory, currently maintained
by @garious. Changing the architecture is a three step process:

1. Propose to a change to the architecture by creating a PR that adds a markdown file called a "wip-doc"
   (meaning "work-in-progress document" and pronounced "whip dock") to the directory `doc/wip/`. Add
   at least the maintainer of `doc/` as a reviewer.
2. The PR being merged indicates your proposed change was accepted. The document will remain in `wip/`
   while additional PRs are submitted to update the Solana reference design, which is the Rust-only
   version of this codebase (no `--features` enabled). Be sure to update the wip-doc as the
   implementation reveals the need for tweaks to the architecture.
3. Once implemented, contact the maintainer of the `doc/` to integrate the wip-doc into `doc/`.
