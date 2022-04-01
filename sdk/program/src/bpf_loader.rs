//! The latest Solana BPF loader.
//!
//! The BPF loader is responsible for loading, finalizing, and executing BPF
//! programs.  Not all networks may support the latest loader.  You can use the
//! command-line tools to check if this version of the loader is supported by
//! requesting the account info for the public key below.
//!
//! The program format may change between loaders, and it is crucial to build
//! your program against the proper entrypoint semantics.  All programs being
//! deployed to this BPF loader must build against the latest entrypoint version
//! located in `entrypoint.rs`.
//!
//! Note: Programs built for older loaders must use a matching entrypoint
//! version.  An example is `bpf_loader_deprecated.rs` which requires
//! `entrypoint_deprecated.rs`.

crate::declare_id!("BPFLoader2111111111111111111111111111111111");
