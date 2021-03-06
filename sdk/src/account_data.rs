use serde::de::{Deserialize, Deserializer};
use serde::ser::{Serialize, Serializer};
use solana_program::sysvar::{Sysvar, SysvarEnum};
use std::{
    any::TypeId,
    collections::{hash_map::Entry, HashMap},
    convert::TryFrom,
    iter::FromIterator,
    ops::{Deref, DerefMut, Index, IndexMut},
    slice::SliceIndex,
    sync::RwLock,
};

#[derive(Debug, Default, AbiExample)]
pub struct AccountData {
    data: Vec<u8>,
    cache: RwLock<HashMap<TypeId, Option<SysvarEnum>>>,
}

impl AccountData {
    /// Reads the sysvar from the serialized data and caches the result.
    /// Following reads of the same sysvar will read from the cache until the
    /// data is mutated.
    pub(crate) fn get_sysvar<S>(&self) -> Option<S>
    where
        S: Sysvar + Clone + Into<SysvarEnum> + TryFrom<SysvarEnum> + 'static,
    {
        let key = TypeId::of::<S>();
        let try_from = |v| S::try_from(v).ok();
        if let Some(val) = self.cache.read().unwrap().get(&key) {
            return val.clone().and_then(try_from);
        }
        // Cache may be modified between above read-lock and below write-lock,
        // so we have to check for Entry::Occupied again.
        match self.cache.write().unwrap().entry(key) {
            Entry::Vacant(entry) => {
                let val = bincode::deserialize::<S>(&self.data).ok();
                entry.insert(val.clone().map(Into::into));
                val
            }
            Entry::Occupied(entry) => entry.get().clone().and_then(try_from),
        }
    }

    /// Serializes the sysvar into the internal bytes buffer.
    pub(crate) fn put_sysvar<S>(&mut self, sysvar: &S) -> bincode::Result<()>
    where
        S: Sysvar + Clone + Into<SysvarEnum> + 'static,
    {
        // serialize_into will not write to the buffer if it fails. So we will
        // update the cache only if this succeeds.
        bincode::serialize_into(&mut self.data[..], sysvar)?;
        let key = std::any::TypeId::of::<S>();
        let mut cache = self.cache.write().unwrap();
        cache.clear(); // Invalidate existing cache.
        cache.insert(key, Some(sysvar.clone().into()));
        Ok(())
    }
}

impl From<Vec<u8>> for AccountData {
    fn from(data: Vec<u8>) -> Self {
        Self {
            data,
            cache: RwLock::default(),
        }
    }
}

impl From<AccountData> for Vec<u8> {
    fn from(account_data: AccountData) -> Self {
        account_data.data
    }
}

impl FromIterator<u8> for AccountData {
    fn from_iter<I>(iter: I) -> Self
    where
        I: IntoIterator<Item = u8>,
    {
        Self::from(iter.into_iter().collect::<Vec<u8>>())
    }
}

impl Deref for AccountData {
    type Target = Vec<u8>;

    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl DerefMut for AccountData {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // Invalidate the cache since the data may be mutated.
        self.cache.write().unwrap().clear();
        &mut self.data
    }
}

impl AsRef<[u8]> for AccountData {
    fn as_ref(&self) -> &[u8] {
        &self.data
    }
}

impl<I> Index<I> for AccountData
where
    I: SliceIndex<[u8]>,
{
    type Output = I::Output;

    #[inline]
    fn index(&self, index: I) -> &Self::Output {
        self.data.index(index)
    }
}

impl<I> IndexMut<I> for AccountData
where
    I: SliceIndex<[u8]>,
{
    #[inline]
    fn index_mut(&mut self, index: I) -> &mut Self::Output {
        // Invalidate the cache since the data may be mutated.
        self.cache.write().unwrap().clear();
        self.data.index_mut(index)
    }
}

impl Clone for AccountData {
    fn clone(&self) -> Self {
        Self {
            data: self.data.clone(),
            cache: RwLock::new(self.cache.read().unwrap().clone()),
        }
    }
}

impl PartialEq<AccountData> for AccountData {
    fn eq(&self, other: &Self) -> bool {
        self.data == other.data
    }
}

impl Eq for AccountData {}

impl Serialize for AccountData {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serde_bytes::serialize(&self.data, serializer)
    }
}

impl<'de> Deserialize<'de> for AccountData {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let data = serde_bytes::deserialize::<Vec<u8>, D>(deserializer)?;
        Ok(Self::from(data))
    }
}

#[cfg(test)]
mod tests {
    // TODO: Add tests.
}
