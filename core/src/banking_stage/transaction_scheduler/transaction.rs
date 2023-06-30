use core::{fmt::Display, hash::Hash};

/// Simple trait to define what an arbitrary transaction is:
/// 1. Read and Write locked resources
pub trait Transaction<R: ResourceKey> {
    fn read_locks(&self) -> &[R];
    fn write_locks(&self) -> &[R];
}

/// Trait alias for resource keys (account keys)
pub trait ResourceKey: Copy + Hash + PartialEq + Eq + Sized + Display + 'static {}
impl<K: Copy + Hash + PartialEq + Eq + Sized + Display + 'static> ResourceKey for K {}
