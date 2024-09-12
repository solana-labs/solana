use serde::{ser::Error, Deserialize, Deserializer, Serialize, Serializer};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OptionSerializer<T> {
    Some(T),
    None,
    Skip,
}

impl<T> OptionSerializer<T> {
    pub fn none() -> Self {
        Self::None
    }

    pub fn skip() -> Self {
        Self::Skip
    }

    pub fn should_skip(&self) -> bool {
        matches!(self, Self::Skip)
    }

    pub fn or_skip(option: Option<T>) -> Self {
        match option {
            Option::Some(item) => Self::Some(item),
            Option::None => Self::Skip,
        }
    }

    pub fn as_ref(&self) -> OptionSerializer<&T> {
        match self {
            OptionSerializer::Some(item) => OptionSerializer::Some(item),
            OptionSerializer::None => OptionSerializer::None,
            OptionSerializer::Skip => OptionSerializer::Skip,
        }
    }

    pub fn as_mut(&mut self) -> OptionSerializer<&mut T> {
        match *self {
            OptionSerializer::Some(ref mut x) => OptionSerializer::Some(x),
            _ => OptionSerializer::None,
        }
    }

    pub fn is_some(&self) -> bool {
        matches!(*self, OptionSerializer::Some(_))
    }

    pub fn is_none(&self) -> bool {
        matches!(*self, OptionSerializer::None)
    }

    pub fn is_skip(&self) -> bool {
        matches!(*self, OptionSerializer::Skip)
    }

    pub fn expect(self, msg: &str) -> T {
        match self {
            OptionSerializer::Some(val) => val,
            _ => panic!("{}", msg),
        }
    }

    pub fn unwrap(self) -> T {
        match self {
            OptionSerializer::Some(val) => val,
            OptionSerializer::None => {
                panic!("called `OptionSerializer::unwrap()` on a `None` value")
            }
            OptionSerializer::Skip => {
                panic!("called `OptionSerializer::unwrap()` on a `Skip` value")
            }
        }
    }

    pub fn unwrap_or(self, default: T) -> T {
        match self {
            OptionSerializer::Some(val) => val,
            _ => default,
        }
    }

    pub fn unwrap_or_else<F>(self, f: F) -> T
    where
        F: FnOnce() -> T,
    {
        match self {
            OptionSerializer::Some(val) => val,
            _ => f(),
        }
    }

    pub fn map<U, F>(self, f: F) -> Option<U>
    where
        F: FnOnce(T) -> U,
    {
        match self {
            OptionSerializer::Some(x) => Some(f(x)),
            _ => None,
        }
    }

    pub fn map_or<U, F>(self, default: U, f: F) -> U
    where
        F: FnOnce(T) -> U,
    {
        match self {
            OptionSerializer::Some(t) => f(t),
            _ => default,
        }
    }

    pub fn map_or_else<U, D, F>(self, default: D, f: F) -> U
    where
        D: FnOnce() -> U,
        F: FnOnce(T) -> U,
    {
        match self {
            OptionSerializer::Some(t) => f(t),
            _ => default(),
        }
    }

    pub fn filter<P>(self, predicate: P) -> Self
    where
        P: FnOnce(&T) -> bool,
    {
        if let OptionSerializer::Some(x) = self {
            if predicate(&x) {
                return OptionSerializer::Some(x);
            }
        }
        OptionSerializer::None
    }

    pub fn ok_or<E>(self, err: E) -> Result<T, E> {
        match self {
            OptionSerializer::Some(v) => Ok(v),
            _ => Err(err),
        }
    }

    pub fn ok_or_else<E, F>(self, err: F) -> Result<T, E>
    where
        F: FnOnce() -> E,
    {
        match self {
            OptionSerializer::Some(v) => Ok(v),
            _ => Err(err()),
        }
    }
}

impl<T> From<Option<T>> for OptionSerializer<T> {
    fn from(option: Option<T>) -> Self {
        match option {
            Option::Some(item) => Self::Some(item),
            Option::None => Self::None,
        }
    }
}

impl<T> From<OptionSerializer<T>> for Option<T> {
    fn from(option: OptionSerializer<T>) -> Self {
        match option {
            OptionSerializer::Some(item) => Self::Some(item),
            _ => Self::None,
        }
    }
}

impl<T: Serialize> Serialize for OptionSerializer<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Some(item) => item.serialize(serializer),
            Self::None => serializer.serialize_none(),
            Self::Skip => Err(Error::custom("Skip variants should not be serialized")),
        }
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for OptionSerializer<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Option::deserialize(deserializer).map(Into::into)
    }
}
