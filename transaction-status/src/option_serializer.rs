use serde::{ser::Error, Deserialize, Deserializer, Serialize, Serializer};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OptionSerializer<T: Default> {
    Some(T),
    None,
    Default,
    Skip,
}

impl<T: Default> Default for OptionSerializer<T> {
    fn default() -> Self {
        Self::None
    }
}

impl<T: Default> OptionSerializer<T> {
    pub fn skip() -> Self {
        Self::Skip
    }

    pub fn should_skip(&self) -> bool {
        matches!(self, Self::Skip)
    }

    pub fn or_default(option: Option<T>) -> Self {
        match option {
            Option::Some(item) => Self::Some(item),
            Option::None => Self::Default,
        }
    }

    pub fn or_skip(option: Option<T>) -> Self {
        match option {
            Option::Some(item) => Self::Some(item),
            Option::None => Self::Skip,
        }
    }
}

impl<T: Default> From<Option<T>> for OptionSerializer<T> {
    fn from(option: Option<T>) -> Self {
        match option {
            Option::Some(item) => Self::Some(item),
            Option::None => Self::None,
        }
    }
}

impl<T: Default> From<OptionSerializer<T>> for Option<T> {
    fn from(option: OptionSerializer<T>) -> Self {
        match option {
            OptionSerializer::Some(item) => Self::Some(item),
            _ => Self::None,
        }
    }
}

impl<T: Default + Serialize> Serialize for OptionSerializer<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Some(item) => item.serialize(serializer),
            Self::None => serializer.serialize_none(),
            Self::Default => T::default().serialize(serializer),
            Self::Skip => Err(Error::custom("Skip variants should not be serialized")),
        }
    }
}

impl<'de, T: Default + Deserialize<'de> + PartialEq> Deserialize<'de> for OptionSerializer<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let option: Option<T> = Deserialize::deserialize(deserializer)?;
        Ok(match option {
            Some(item) => {
                if item == T::default() {
                    Self::Default
                } else {
                    Self::Some(item)
                }
            }
            None => Self::None,
        })
    }
}
