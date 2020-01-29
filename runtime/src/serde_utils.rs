use std::{
    fmt,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};

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

pub fn deserialize_atomicu64<'de, D>(d: D) -> Result<AtomicU64, D::Error>
where
    D: serde::de::Deserializer<'de>,
{
    let value = d.deserialize_u64(U64Visitor)?;
    Ok(AtomicU64::new(value))
}

pub fn serialize_atomicu64<S>(x: &AtomicU64, s: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    s.serialize_u64(x.load(Ordering::Relaxed))
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
    s.serialize_bool(x.load(Ordering::Relaxed))
}
