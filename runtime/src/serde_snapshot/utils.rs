use serde::{
    ser::{SerializeSeq, SerializeTuple},
    Serialize, Serializer,
};
#[cfg(all(test, RUSTC_WITH_SPECIALIZATION))]
use solana_frozen_abi::abi_example::IgnoreAsHelper;

// consumes an iterator and returns an object that will serialize as a serde seq
#[allow(dead_code)]
pub fn serialize_iter_as_seq<I>(iter: I) -> impl Serialize
where
    I: IntoIterator,
    <I as IntoIterator>::Item: Serialize,
    <I as IntoIterator>::IntoIter: ExactSizeIterator,
{
    struct SerializableSequencedIterator<I> {
        iter: std::cell::RefCell<Option<I>>,
    }

    #[cfg(all(test, RUSTC_WITH_SPECIALIZATION))]
    impl<I> IgnoreAsHelper for SerializableSequencedIterator<I> {}

    impl<I> Serialize for SerializableSequencedIterator<I>
    where
        I: IntoIterator,
        <I as IntoIterator>::Item: Serialize,
        <I as IntoIterator>::IntoIter: ExactSizeIterator,
    {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            let iter = self.iter.borrow_mut().take().unwrap().into_iter();
            let mut seq = serializer.serialize_seq(Some(iter.len()))?;
            for item in iter {
                seq.serialize_element(&item)?;
            }
            seq.end()
        }
    }

    SerializableSequencedIterator {
        iter: std::cell::RefCell::new(Some(iter)),
    }
}

// consumes an iterator and returns an object that will serialize as a serde tuple
#[allow(dead_code)]
pub fn serialize_iter_as_tuple<I>(iter: I) -> impl Serialize
where
    I: IntoIterator,
    <I as IntoIterator>::Item: Serialize,
    <I as IntoIterator>::IntoIter: ExactSizeIterator,
{
    struct SerializableSequencedIterator<I> {
        iter: std::cell::RefCell<Option<I>>,
    }

    #[cfg(all(test, RUSTC_WITH_SPECIALIZATION))]
    impl<I> IgnoreAsHelper for SerializableSequencedIterator<I> {}

    impl<I> Serialize for SerializableSequencedIterator<I>
    where
        I: IntoIterator,
        <I as IntoIterator>::Item: Serialize,
        <I as IntoIterator>::IntoIter: ExactSizeIterator,
    {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            let iter = self.iter.borrow_mut().take().unwrap().into_iter();
            let mut tup = serializer.serialize_tuple(iter.len())?;
            for item in iter {
                tup.serialize_element(&item)?;
            }
            tup.end()
        }
    }

    SerializableSequencedIterator {
        iter: std::cell::RefCell::new(Some(iter)),
    }
}

// consumes a 2-tuple iterator and returns an object that will serialize as a serde map
#[allow(dead_code)]
pub fn serialize_iter_as_map<K, V, I>(iter: I) -> impl Serialize
where
    K: Serialize,
    V: Serialize,
    I: IntoIterator<Item = (K, V)>,
{
    struct SerializableMappedIterator<I> {
        iter: std::cell::RefCell<Option<I>>,
    }

    #[cfg(all(test, RUSTC_WITH_SPECIALIZATION))]
    impl<I> IgnoreAsHelper for SerializableMappedIterator<I> {}

    impl<K, V, I> Serialize for SerializableMappedIterator<I>
    where
        K: Serialize,
        V: Serialize,
        I: IntoIterator<Item = (K, V)>,
    {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            serializer.collect_map(self.iter.borrow_mut().take().unwrap())
        }
    }

    SerializableMappedIterator {
        iter: std::cell::RefCell::new(Some(iter)),
    }
}
