use crate::abi_digester::{AbiDigester, DigestError, DigestResult};

use log::*;

use serde::Serialize;
use std::any::type_name;

pub trait AbiExample: Sized {
    fn example() -> Self;
}

// Following code snippets are copied and adapted from the official rustc implementation to
// implement AbiExample trait for most of basic types.
// These are licensed under Apache-2.0 + MIT (compatible because we're Apache-2.0)

// Source: https://github.com/rust-lang/rust/blob/ba18875557aabffe386a2534a1aa6118efb6ab88/src/libcore/tuple.rs#L7
macro_rules! tuple_example_impls {
    ($(
        $Tuple:ident {
            $(($idx:tt) -> $T:ident)+
        }
    )+) => {
        $(
            impl<$($T:AbiExample),+> AbiExample for ($($T,)+) {
                fn example() -> Self {
                        ($({ let x: $T = AbiExample::example(); x},)+)
                }
            }
        )+
    }
}

// Source: https://github.com/rust-lang/rust/blob/ba18875557aabffe386a2534a1aa6118efb6ab88/src/libcore/tuple.rs#L110
tuple_example_impls! {
    Tuple1 {
        (0) -> A
    }
    Tuple2 {
        (0) -> A
        (1) -> B
    }
    Tuple3 {
        (0) -> A
        (1) -> B
        (2) -> C
    }
    Tuple4 {
        (0) -> A
        (1) -> B
        (2) -> C
        (3) -> D
    }
    Tuple5 {
        (0) -> A
        (1) -> B
        (2) -> C
        (3) -> D
        (4) -> E
    }
    Tuple6 {
        (0) -> A
        (1) -> B
        (2) -> C
        (3) -> D
        (4) -> E
        (5) -> F
    }
    Tuple7 {
        (0) -> A
        (1) -> B
        (2) -> C
        (3) -> D
        (4) -> E
        (5) -> F
        (6) -> G
    }
    Tuple8 {
        (0) -> A
        (1) -> B
        (2) -> C
        (3) -> D
        (4) -> E
        (5) -> F
        (6) -> G
        (7) -> H
    }
    Tuple9 {
        (0) -> A
        (1) -> B
        (2) -> C
        (3) -> D
        (4) -> E
        (5) -> F
        (6) -> G
        (7) -> H
        (8) -> I
    }
    Tuple10 {
        (0) -> A
        (1) -> B
        (2) -> C
        (3) -> D
        (4) -> E
        (5) -> F
        (6) -> G
        (7) -> H
        (8) -> I
        (9) -> J
    }
    Tuple11 {
        (0) -> A
        (1) -> B
        (2) -> C
        (3) -> D
        (4) -> E
        (5) -> F
        (6) -> G
        (7) -> H
        (8) -> I
        (9) -> J
        (10) -> K
    }
    Tuple12 {
        (0) -> A
        (1) -> B
        (2) -> C
        (3) -> D
        (4) -> E
        (5) -> F
        (6) -> G
        (7) -> H
        (8) -> I
        (9) -> J
        (10) -> K
        (11) -> L
    }
}

// Source: https://github.com/rust-lang/rust/blob/ba18875557aabffe386a2534a1aa6118efb6ab88/src/libcore/array/mod.rs#L417
macro_rules! array_example_impls {
    {$n:expr, $t:ident $($ts:ident)*} => {
        impl<T> AbiExample for [T; $n] where T: AbiExample {
            fn example() -> Self {
                [$t::example(), $($ts::example()),*]
            }
        }
        array_example_impls!{($n - 1), $($ts)*}
    };
    {$n:expr,} => {
        impl<T> AbiExample for [T; $n] {
        fn example() -> Self { [] }
        }
    };
}

array_example_impls! {32, T T T T T T T T T T T T T T T T T T T T T T T T T T T T T T T T}

// Source: https://github.com/rust-lang/rust/blob/ba18875557aabffe386a2534a1aa6118efb6ab88/src/libcore/default.rs#L137
macro_rules! example_impls {
    ($t:ty, $v:expr) => {
        impl AbiExample for $t {
            fn example() -> Self {
                $v
            }
        }
    };
}

example_impls! { (), () }
example_impls! { bool, false }
example_impls! { char, '\x00' }

example_impls! { usize, 0 }
example_impls! { u8, 0 }
example_impls! { u16, 0 }
example_impls! { u32, 0 }
example_impls! { u64, 0 }
example_impls! { u128, 0 }

example_impls! { isize, 0 }
example_impls! { i8, 0 }
example_impls! { i16, 0 }
example_impls! { i32, 0 }
example_impls! { i64, 0 }
example_impls! { i128, 0 }

example_impls! { f32, 0.0f32 }
example_impls! { f64, 0.0f64 }
example_impls! { String, String::new() }
example_impls! { std::time::Duration, std::time::Duration::from_secs(0) }

use std::sync::atomic::*;

// Source: https://github.com/rust-lang/rust/blob/ba18875557aabffe386a2534a1aa6118efb6ab88/src/libcore/sync/atomic.rs#L1199
macro_rules! atomic_example_impls {
    ($atomic_type: ident) => {
        impl AbiExample for $atomic_type {
            fn example() -> Self {
                Self::new(AbiExample::example())
            }
        }
    };
}
atomic_example_impls! { AtomicU8 }
atomic_example_impls! { AtomicU16 }
atomic_example_impls! { AtomicU32 }
atomic_example_impls! { AtomicU64 }
atomic_example_impls! { AtomicUsize }
atomic_example_impls! { AtomicI8 }
atomic_example_impls! { AtomicI16 }
atomic_example_impls! { AtomicI32 }
atomic_example_impls! { AtomicI64 }
atomic_example_impls! { AtomicIsize }
atomic_example_impls! { AtomicBool }

#[cfg(not(feature = "program"))]
use generic_array::{ArrayLength, GenericArray};
#[cfg(not(feature = "program"))]
impl<T: Default, U: ArrayLength<T>> AbiExample for GenericArray<T, U> {
    fn example() -> Self {
        Self::default()
    }
}

use bv::{BitVec, BlockType};
impl<T: BlockType> AbiExample for BitVec<T> {
    fn example() -> Self {
        Self::default()
    }
}

impl<T: BlockType> IgnoreAsHelper for BitVec<T> {}
impl<T: BlockType> EvenAsOpaque for BitVec<T> {}

pub(crate) fn normalize_type_name(type_name: &str) -> String {
    type_name.chars().filter(|c| *c != '&').collect()
}

type Placeholder = ();

impl<T: Sized> AbiExample for T {
    default fn example() -> Self {
        <Placeholder>::type_erased_example()
    }
}

// this works like a type erasure and a hatch to escape type error to runtime error
trait TypeErasedExample<T> {
    fn type_erased_example() -> T;
}

impl<T: Sized> TypeErasedExample<T> for Placeholder {
    default fn type_erased_example() -> T {
        panic!(
            "derive or implement AbiExample/AbiEnumVisitor for {}",
            type_name::<T>()
        );
    }
}

impl<T: Default + Serialize> TypeErasedExample<T> for Placeholder {
    default fn type_erased_example() -> T {
        let original_type_name = type_name::<T>();
        let normalized_type_name = normalize_type_name(original_type_name);

        if normalized_type_name.starts_with("solana") {
            panic!(
                "derive or implement AbiExample/AbiEnumVisitor for {}",
                original_type_name
            );
        } else {
            panic!(
                "new unrecognized type for ABI digest!: {}",
                original_type_name
            )
        }
    }
}

impl<T: AbiExample> AbiExample for Option<T> {
    fn example() -> Self {
        info!("AbiExample for (Option<T>): {}", type_name::<Self>());
        Some(T::example())
    }
}

impl<O: AbiExample, E: AbiExample> AbiExample for Result<O, E> {
    fn example() -> Self {
        info!("AbiExample for (Result<O, E>): {}", type_name::<Self>());
        Ok(O::example())
    }
}

impl<T: AbiExample> AbiExample for Box<T> {
    fn example() -> Self {
        info!("AbiExample for (Box<T>): {}", type_name::<Self>());
        Box::new(T::example())
    }
}

impl<T> AbiExample for Box<dyn Fn(&mut T) + Sync + Send> {
    fn example() -> Self {
        info!("AbiExample for (Box<T>): {}", type_name::<Self>());
        Box::new(move |_t: &mut T| {})
    }
}

impl<T: AbiExample> AbiExample for Box<[T]> {
    fn example() -> Self {
        info!("AbiExample for (Box<[T]>): {}", type_name::<Self>());
        Box::new([T::example()])
    }
}

impl<T: AbiExample> AbiExample for std::marker::PhantomData<T> {
    fn example() -> Self {
        info!("AbiExample for (PhantomData<T>): {}", type_name::<Self>());
        <std::marker::PhantomData<T>>::default()
    }
}

impl<T: AbiExample> AbiExample for std::sync::Arc<T> {
    fn example() -> Self {
        info!("AbiExample for (Arc<T>): {}", type_name::<Self>());
        std::sync::Arc::new(T::example())
    }
}

impl<T: AbiExample> AbiExample for std::rc::Rc<T> {
    fn example() -> Self {
        info!("AbiExample for (Rc<T>): {}", type_name::<Self>());
        std::rc::Rc::new(T::example())
    }
}

impl<T: AbiExample> AbiExample for std::sync::Mutex<T> {
    fn example() -> Self {
        info!("AbiExample for (Mutex<T>): {}", type_name::<Self>());
        std::sync::Mutex::new(T::example())
    }
}

impl<T: AbiExample> AbiExample for std::sync::RwLock<T> {
    fn example() -> Self {
        info!("AbiExample for (RwLock<T>): {}", type_name::<Self>());
        std::sync::RwLock::new(T::example())
    }
}

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

impl<
        T: std::cmp::Eq + std::hash::Hash + AbiExample,
        S: AbiExample,
        H: std::hash::BuildHasher + Default,
    > AbiExample for HashMap<T, S, H>
{
    fn example() -> Self {
        info!("AbiExample for (HashMap<T, S, H>): {}", type_name::<Self>());
        let mut map = HashMap::default();
        map.insert(T::example(), S::example());
        map
    }
}

impl<T: std::cmp::Ord + AbiExample, S: AbiExample> AbiExample for BTreeMap<T, S> {
    fn example() -> Self {
        info!("AbiExample for (BTreeMap<T, S>): {}", type_name::<Self>());
        let mut map = BTreeMap::default();
        map.insert(T::example(), S::example());
        map
    }
}

impl<T: AbiExample> AbiExample for Vec<T> {
    fn example() -> Self {
        info!("AbiExample for (Vec<T>): {}", type_name::<Self>());
        vec![T::example()]
    }
}

impl<T: std::cmp::Eq + std::hash::Hash + AbiExample, H: std::hash::BuildHasher + Default> AbiExample
    for HashSet<T, H>
{
    fn example() -> Self {
        info!("AbiExample for (HashSet<T, H>): {}", type_name::<Self>());
        let mut set: HashSet<T, H> = HashSet::default();
        set.insert(T::example());
        set
    }
}

impl<T: std::cmp::Ord + AbiExample> AbiExample for BTreeSet<T> {
    fn example() -> Self {
        info!("AbiExample for (BTreeSet<T>): {}", type_name::<Self>());
        let mut set: BTreeSet<T> = BTreeSet::default();
        set.insert(T::example());
        set
    }
}

#[cfg(not(feature = "program"))]
impl AbiExample for memmap::MmapMut {
    fn example() -> Self {
        memmap::MmapMut::map_anon(1).expect("failed to map the data file")
    }
}

#[cfg(not(feature = "program"))]
impl AbiExample for std::path::PathBuf {
    fn example() -> Self {
        std::path::PathBuf::from(String::example())
    }
}

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
impl AbiExample for SocketAddr {
    fn example() -> Self {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0)
    }
}

// This is a control flow indirection needed for digesting all variants of an enum
pub trait AbiEnumVisitor: Serialize {
    fn visit_for_abi(&self, digester: &mut AbiDigester) -> DigestResult;
}

pub trait IgnoreAsHelper {}
pub trait EvenAsOpaque {}

impl<T: Serialize + ?Sized> AbiEnumVisitor for T {
    default fn visit_for_abi(&self, _digester: &mut AbiDigester) -> DigestResult {
        unreachable!(
            "AbiEnumVisitor must be implemented for {}",
            type_name::<T>()
        );
    }
}

impl<T: Serialize + ?Sized + AbiExample> AbiEnumVisitor for T {
    default fn visit_for_abi(&self, digester: &mut AbiDigester) -> DigestResult {
        info!("AbiEnumVisitor for (default): {}", type_name::<T>());
        T::example()
            .serialize(digester.create_new())
            .map_err(DigestError::wrap_by_type::<T>)
    }
}

// even (experimental) rust specialization isn't enough for us, resort to
// the autoref hack: https://github.com/dtolnay/case-studies/blob/master/autoref-specialization/README.md
// relevant test: TestVecEnum
impl<T: Serialize + ?Sized + AbiEnumVisitor> AbiEnumVisitor for &T {
    default fn visit_for_abi(&self, digester: &mut AbiDigester) -> DigestResult {
        info!("AbiEnumVisitor for (&default): {}", type_name::<T>());
        // Don't call self.visit_for_abi(...) to avoid the infinite recursion!
        T::visit_for_abi(&self, digester)
    }
}

// force to call self.serialize instead of T::visit_for_abi() for serialization
// helper structs like ad-hoc iterator `struct`s
impl<T: Serialize + IgnoreAsHelper> AbiEnumVisitor for &T {
    default fn visit_for_abi(&self, digester: &mut AbiDigester) -> DigestResult {
        info!("AbiEnumVisitor for (IgnoreAsHelper): {}", type_name::<T>());
        self.serialize(digester.create_new())
            .map_err(DigestError::wrap_by_type::<T>)
    }
}

// force to call self.serialize instead of T::visit_for_abi() to work around the
// inability of implementing AbiExample for private structs from other crates
impl<T: Serialize + IgnoreAsHelper + EvenAsOpaque> AbiEnumVisitor for &T {
    default fn visit_for_abi(&self, digester: &mut AbiDigester) -> DigestResult {
        info!("AbiEnumVisitor for (IgnoreAsOpaque): {}", type_name::<T>());
        let top_scope = type_name::<T>().split("::").next().unwrap();
        self.serialize(digester.create_new_opaque(top_scope))
            .map_err(DigestError::wrap_by_type::<T>)
    }
}

// Because Option and Result enums are so common enums, provide generic trait implementations
// The digesting pattern must match with what is derived from #[derive(AbiEnumVisitor)]
impl<T: AbiEnumVisitor> AbiEnumVisitor for Option<T> {
    fn visit_for_abi(&self, digester: &mut AbiDigester) -> DigestResult {
        info!("AbiEnumVisitor for (Option<T>): {}", type_name::<Self>());

        let variant: Self = Option::Some(T::example());
        // serde calls serialize_some(); not serialize_variant();
        // so create_new is correct, not create_enum_child or create_enum_new
        variant.serialize(digester.create_new())
    }
}

impl<O: AbiEnumVisitor, E: AbiEnumVisitor> AbiEnumVisitor for Result<O, E> {
    fn visit_for_abi(&self, digester: &mut AbiDigester) -> DigestResult {
        info!("AbiEnumVisitor for (Result<O, E>): {}", type_name::<Self>());

        digester.update(&["enum Result (variants = 2)"]);
        let variant: Self = Result::Ok(O::example());
        variant.serialize(digester.create_enum_child())?;

        let variant: Self = Result::Err(E::example());
        variant.serialize(digester.create_enum_child())?;

        Ok(digester.create_child())
    }
}
