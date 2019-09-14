
Cargo (as of 1.37) does not nicely handle the scenario where features of a
dependent crate of multiple other crates in the tree vary.

To illustrate the problem consider crates A, B and C arranged as follows:
* Crate A and B are both members of a Cargo virtual manifest
* Crate C provides two features, F1 and F2
* Crate A requests feature F1 of C, crate B requests F2 of C

When crate A and B are built together, `cargo` builds C with both feature F1 and
F2 enabled (the union of all enabled features).  However when A or B are built
individually, `cargo` builds C with only feature F1 or F2 enabled.

Unfortunately in all these cases, `cargo` will build crate C in the same target
location and the outputs for C will be recreated every time the features for crate C
change.

From a clean workspace, building A individually first will cause C to be built
as expected.  Then build B individually and C will be re-built because F2 was
enabled instead of F1.  Now rebuild A and observe that C will be re-built again
because F1 was re-enabled.

In practice this problem is much less obvious as both A and B likely to not have
a direct dependency on C, indirectly causing rebuilds of numerous other crates as well.

The `solana-crate-features` offers a workaround to this "feature thrashing"
problem by explicitly declaring all "C-like crates" with the union of all features
that any other crate in the tree (either explicitly or implicitly) enable.  All
crates in the Solana source tree should depend on `solana-crate-features`.

### Adding new dependent crates
When unnecessary `cargo` rebuilds are observed, the first step is to figure what
dependent crate is suffering from feature thrashing.

This information is not readily available from the stock `cargo` program, so use
the following steps to produce a custom `cargo` program that will output the
necessary feature information to stderr during a build:
```bash
$ git clone git@github.com:rust-lang/cargo.git -b rust-1.37.0
$ cd cargo
$ git apply 0001-Print-package-features.patch
$ cargo build
$ mv ~/.cargo/bin/cargo ~/.cargo/bin/cargo.org
$ cp ./target/debug/cargo ~/.cargo/bin/cargo
```

Rebuild with the custom `cargo` and search for indications of crates getting
built with different features (repeated runs of `./scripts/cargo-install-all.sh`
work great for this).  When the problematic crate is identified, add it as a
dependency of `solana-crate-features` with the union of all observed enabled
features for that crate.

### Appendix
This command will enable some additional cargo log output that can help debug
dependency problems as well:
```bash
export CARGO_LOG=cargo::core::compiler::fingerprint=info
```
