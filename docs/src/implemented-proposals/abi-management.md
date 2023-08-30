---
title: Solana ABI management process
---

This document proposes the Solana ABI management process. The ABI management
process is an engineering practice and a supporting technical framework to avoid
introducing unintended incompatible ABI changes.

# Problem

The Solana ABI (binary interface to the cluster) is currently only defined
implicitly by the implementation and requires a very careful eye to notice
breaking changes. This makes it extremely difficult to upgrade the software
on an existing cluster without rebooting the ledger.

# Requirements and objectives

- Unintended ABI changes can be detected as CI failures mechanically.
- Newer implementation must be able to process the oldest data (since genesis)
  once we go mainnet.
- The objective of this proposal is to protect the ABI while sustaining rather
  rapid development by opting for a mechanical process rather than a very long
  human-driven auditing process.
- Once signed cryptographically, data blob must be identical, so no
  in-place data format update is possible regardless of inbound and outbound of
  the online system. Also, considering the sheer volume of transactions we're
  aiming to handle, retrospective in-place update is undesirable at best.

# Solution

Instead of natural human's eye due-diligence, which should be assumed to fail
regularly, we need a systematic assurance of not breaking the cluster when
changing the source code.

For that purpose, we introduce a mechanism of marking every ABI-related things
in source code (`struct`s, `enum`s) with the new `#[frozen_abi]` attribute. This
takes hard-coded digest value derived from types of its fields via
`ser::Serialize`. And the attribute automatically generates a unit test to try
to detect any unsanctioned changes to the marked ABI-related things.

However, the detection cannot be complete; no matter how hard we statically
analyze the source code, it's still possible to break ABI. For example, this
includes not-`derive`d hand-written `ser::Serialize`, underlying library's
implementation changes (for example `bincode`), CPU architecture differences.
The detection of these possible ABI incompatibilities is out-of-scope for this
ABI management.

# Definitions

ABI item/type: various types to be used for serialization, which collectively
comprises the whole ABI for any system components. For example, those types
include `struct`s and `enum`s.

ABI item digest: Some fixed hash derived from type information of ABI item's
fields.

# Example

```patch
+#[frozen_abi(digest="eXSMM7b89VY72V...")]
 #[derive(Serialize, Default, Deserialize, Debug, PartialEq, Eq, Clone)]
 pub struct Vote {
     /// A stack of votes starting with the oldest vote
     pub slots: Vec<Slot>,
     /// signature of the bank's state at the last slot
     pub hash: Hash,
 }
```

# Developer's workflow

To know the digest for new ABI items, developers can add `frozen_abi` with a
random digest value and run the unit tests and replace it with the correct
digest from the assertion test error message.

Run unit tests using the following command to generate digest values:
```
SOLANA_ABI_DUMP_DIR=. cargo +nightly test abi
```

In general, once we add `frozen_abi` and its change is published in the stable
release channel, its digest should never change. If such a change is needed, we
should opt for defining a new `struct` like `FooV1`. And special release flow
like hard forks should be approached.

# Implementation remarks

We use some degree of macro machinery to automatically generate unit tests
and calculate a digest from ABI items. This is doable by clever use of
`serde::Serialize` (`[1]`) and `any::type_name` (`[2]`). For a precedent for similar
implementation, `ink` from the Parity Technologies `[3]` could be informational.

# Implementation details

The implementation's goal is to detect unintended ABI changes automatically as
much as possible. To that end, the digest of structural ABI information is
calculated with best-effort accuracy and stability.

When the ABI digest check is run, it dynamically computes an ABI digest by
recursively digesting the ABI of fields of the ABI item, by re-using the
`serde`'s serialization functionality, proc macro and generic specialization.
And then, the check `assert!`s that its finalized digest value is identical as
what is specified in the `frozen_abi` attribute.

To realize that, it creates an example instance of the type and a custom
`Serializer` instance for `serde` to recursively traverse its fields as if
serializing the example for real. This traversing must be done via `serde` to
really capture what kinds of data actually would be serialized by `serde`, even
considering custom non-`derive`d `Serialize` trait implementations.

# The ABI digesting process

This part is a bit complex. There is three inter-depending parts: `AbiExample`,
`AbiDigester` and `AbiEnumVisitor`.

First, the generated test creates an example instance of the digested type with
a trait called `AbiExample`, which should be implemented for all of digested
types like the `Serialize` and return `Self` like the `Default` trait. Usually,
it's provided via generic trait specialization for most of common types. Also
it is possible to `derive` for `struct` and `enum` and can be hand-written if
needed.

The custom `Serializer` is called `AbiDigester`. And when it's called by `serde`
to serialize some data, it recursively collects ABI information as much as
possible. `AbiDigester`'s internal state for the ABI digest is updated
differentially depending on the type of data. This logic is specifically
redirected via with a trait called `AbiEnumVisitor` for each `enum` type. As the
name suggests, there is no need to implement `AbiEnumVisitor` for other types.

To summarize this interplay, `serde` handles the recursive serialization control
flow in tandem with `AbiDigester`. The initial entry point in tests and child
`AbiDigester`s use `AbiExample` recursively to create an example object
hierarchal graph. And `AbiDigester` uses `AbiEnumVisitor` to inquiry the actual
ABI information using the constructed sample.

`Default` isn't enough for `AbiExample`. Various collection's `::default()` is
empty, yet, we want to digest them with actual items. And, ABI digesting can't
be realized only with `AbiEnumVisitor`. `AbiExample` is required because an
actual instance of type is needed to actually traverse the data via `serde`.

On the other hand, ABI digesting can't be done only with `AbiExample`, either.
`AbiEnumVisitor` is required because all variants of an `enum` cannot be
traversed just with a single variant of it as a ABI example.

Digestable information:

- rust's type name
- `serde`'s data type name
- all fields in `struct`
- all variants in `enum`
- `struct`: normal(`struct {...}`) and tuple-style (`struct(...)`)
- `enum`: normal variants and `struct`- and `tuple`- styles.
- attributes: `serde(serialize_with=...)` and `serde(skip)`

Not digestable information:

- Any custom serialize code path not touched by the sample provided by
  `AbiExample`. (technically not possible)
- generics (must be a concrete type; use `frozen_abi` on concrete type
  aliases)

# References

1. [(De)Serialization with type info · Issue #1095 · serde-rs/serde](https://github.com/serde-rs/serde/issues/1095#issuecomment-345483479)
2. [`std::any::type_name` - Rust](https://doc.rust-lang.org/std/any/fn.type_name.html)
3. [Parity's ink to write smart contracts](https://github.com/paritytech/ink)
