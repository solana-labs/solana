---
title: Rust Clients
---

## Problem

High-level tests, such as bench-tps, are written in terms of the `Client`
trait. When we execute these tests as part of the test suite, we use the
low-level `BankClient` implementation. When we need to run the same test
against a cluster, we use the `ThinClient` implementation. The problem with
that approach is that it means the trait will continually expand to include new
utility functions and all implementations of it need to add the new
functionality. By separating the user-facing object from the trait that abstracts
the network interface, we can expand the user-facing object to include all sorts
of useful functionality, such as the "spinner" from RpcClient, without concern
for needing to extend the trait and its implementations.

## Proposed Solution

Instead of implementing the `Client` trait, `ThinClient` should be constructed
with an implementation of it. That way, all utility functions currently in the
`Client` trait can move into `ThinClient`. `ThinClient` could then move into
`solana-sdk` since all its network dependencies would be in the implementation
of `Client`. We would then add a new implementation of `Client`, called
`ClusterClient`, and that would live in the `solana-client` crate, where
`ThinClient` currently resides.

After this reorg, any code needing a client would be written in terms of
`ThinClient`. In unit tests, the functionality would be invoked with
`ThinClient<BankClient>`, whereas `main()` functions, benchmarks and
integration tests would invoke it with `ThinClient<ClusterClient>`.

If higher-level components require more functionality than what could be
implemented by `BankClient`, it should be implemented by a second object
that implements a second trait, following the same pattern described here.

### Error Handling

The `Client` should use the existing `TransportError` enum for errors, except
that the `Custom(String)` field should be changed to `Custom(Box<dyn Error>)`.

### Implementation Strategy

1. Add new object to `solana-sdk`, `RpcClientTng`, where the `Tng` suffix is
   temporary and stands for "The Next Generation"
2. Initialize `RpcClientTng` with a `SyncClient` implementation.
3. Add new object to `solana-sdk`, `ThinClientTng`; initialize it with
   `RpcClientTng` and an `AsyncClient` implementation
4. Move all unit-tests from `BankClient` to `ThinClientTng<BankClient>`
5. Add `ClusterClient`
6. Move `ThinClient` users to `ThinClientTng<ClusterClient>`
7. Delete `ThinClient` and rename `ThinClientTng` to `ThinClient`
8. Move `RpcClient` users to new `ThinClient<ClusterClient>`
9. Delete `RpcClient` and rename `RpcClientTng` to `RpcClient`
