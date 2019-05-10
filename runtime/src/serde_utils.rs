use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

struct U64Visitor;
impl<'a> serde::de::Visitor<'a> for U64Visitor {
    type Value = u64;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("Expecting u64")
    }
    fn visit_u64<E>(self, data: u64) -> std::result::Result<u64, E>
    where
        E: serde::de::Error,
    {
        Ok(data)
    }
}

pub fn deserialize_atomicusize<'de, D>(d: D) -> Result<AtomicUsize, D::Error>
where
    D: serde::de::Deserializer<'de>,
{
    let value = d.deserialize_u64(U64Visitor)?;
    Ok(AtomicUsize::new(value as usize))
}

pub fn serialize_atomicusize<S>(x: &AtomicUsize, s: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    s.serialize_u64(x.load(Ordering::SeqCst) as u64)
}

struct BoolVisitor;
impl<'a> serde::de::Visitor<'a> for BoolVisitor {
    type Value = bool;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("Expecting bool")
    }
    fn visit_bool<E>(self, data: bool) -> std::result::Result<bool, E>
    where
        E: serde::de::Error,
    {
        Ok(data)
    }
}

pub fn deserialize_atomicbool<'de, D>(d: D) -> Result<AtomicBool, D::Error>
where
    D: serde::de::Deserializer<'de>,
{
    let value = d.deserialize_bool(BoolVisitor)?;
    Ok(AtomicBool::new(value))
}

pub fn serialize_atomicbool<S>(x: &AtomicBool, s: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    s.serialize_bool(x.load(Ordering::SeqCst))
}
