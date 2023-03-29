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
