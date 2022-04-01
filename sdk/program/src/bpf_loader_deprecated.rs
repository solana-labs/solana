//! The original and now deprecated Solana BPF loader.
//!
//! The BPF loader is responsible for loading, finalizing, and executing BPF
//! programs.
//!
//! This loader is deprecated, and it is strongly encouraged to build for and
//! deploy to the latest BPF loader.  For more information see `bpf_loader.rs`
//!
//! The program format may change between loaders, and it is crucial to build
//! your program against the proper entrypoint semantics.  All programs being
//! deployed to this BPF loader must build against the deprecated entrypoint
//! version located in `entrypoint_deprecated.rs`.

crate::declare_id!("BPFLoader1111111111111111111111111111111111");
