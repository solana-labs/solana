///
/// This lib contains both standard imports and imports shuttle.
/// Shuttle is a Rust crate that facilitates multithreaded testing. It has its own scheduler
/// and can efficiently detect bugs in concurrent code. The downside is that we need to replace
/// all imports by those from Shuttle.
///
/// Instead of importing from std, rand, and so on, import the following from solana-type-override,
/// and include the 'shuttle-test' feature in your crate to use shuttle.

#[cfg(feature = "executor")]
pub mod executor {
    #[cfg(not(feature = "shuttle-test"))]
    pub use futures::executor::*;
    #[cfg(feature = "shuttle-test")]
    pub use shuttle::future::*;
}

pub mod hint {
    #[cfg(feature = "shuttle-test")]
    pub use shuttle::hint::*;
    #[cfg(not(feature = "shuttle-test"))]
    pub use std::hint::*;
}

pub mod lazy_static {
    #[cfg(not(feature = "shuttle-test"))]
    pub use lazy_static::*;
    #[cfg(feature = "shuttle-test")]
    pub use shuttle::lazy_static::*;
}

pub mod rand {
    pub use rand::*;
    #[cfg(feature = "shuttle-test")]
    pub use shuttle::rand::{thread_rng, Rng, RngCore};
}

pub mod sync {
    #[cfg(feature = "shuttle-test")]
    pub use shuttle::sync::*;
    #[cfg(not(feature = "shuttle-test"))]
    pub use std::sync::*;
}

pub mod thread {
    #[cfg(feature = "shuttle-test")]
    pub use shuttle::thread::*;
    #[cfg(not(feature = "shuttle-test"))]
    pub use std::thread::*;
}
