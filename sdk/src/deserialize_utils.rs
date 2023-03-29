//! Serde helpers.

use serde::{Deserialize, Deserializer};

/// This helper function enables successful deserialization of versioned structs; new structs may
/// include additional fields if they impl Default and are added to the end of the struct. Right
/// now, this function is targeted at `bincode` deserialization; the error match may need to be
/// updated if another package needs to be used in the future.
pub fn default_on_eof<'de, T, D>(d: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    let result = T::deserialize(d);
    ignore_eof_error::<'de, T, D::Error>(result)
}

pub fn ignore_eof_error<'de, T, D>(result: Result<T, D>) -> Result<T, D>
where
    T: Deserialize<'de> + Default,
    D: std::fmt::Display,
{
    match result {
        Err(err) if err.to_string() == "io error: unexpected end of file" => Ok(T::default()),
        Err(err) if err.to_string() == "io error: failed to fill whole buffer" => Ok(T::default()),
        result => result,
    }
}

#[cfg(test)]
pub mod tests {
    use {super::*, bincode::deserialize};

    #[test]
    fn test_default_on_eof() {
        #[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
        struct Foo {
            bar: u16,
            #[serde(deserialize_with = "default_on_eof")]
            baz: Option<u16>,
            #[serde(deserialize_with = "default_on_eof")]
            quz: String,
        }

        let data = vec![1, 0];
        assert_eq!(
            Foo {
                bar: 1,
                baz: None,
                quz: "".to_string(),
            },
            deserialize(&data).unwrap()
        );

        let data = vec![1, 0, 0];
        assert_eq!(
            Foo {
                bar: 1,
                baz: None,
                quz: "".to_string(),
            },
            deserialize(&data).unwrap()
        );

        let data = vec![1, 0, 1];
        assert_eq!(
            Foo {
                bar: 1,
                baz: None,
                quz: "".to_string(),
            },
            deserialize(&data).unwrap()
        );

        let data = vec![1, 0, 1, 0];
        assert_eq!(
            Foo {
                bar: 1,
                baz: None,
                quz: "".to_string(),
            },
            deserialize(&data).unwrap()
        );

        let data = vec![1, 0, 1, 0, 0, 1];
        assert_eq!(
            Foo {
                bar: 1,
                baz: Some(0),
                quz: "".to_string(),
            },
            deserialize(&data).unwrap()
        );

        let data = vec![1, 0, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 116];
        assert_eq!(
            Foo {
                bar: 1,
                baz: Some(0),
                quz: "t".to_string(),
            },
            deserialize(&data).unwrap()
        );
    }

    #[test]
    #[should_panic]
    fn test_default_on_eof_additional_untagged_fields() {
        // If later fields are not tagged `deserialize_with = "default_on_eof"`, deserialization
        // will panic on any missing fields/data
        #[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
        struct Foo {
            bar: u16,
            #[serde(deserialize_with = "default_on_eof")]
            baz: Option<u16>,
            quz: String,
        }

        // Fully populated struct will deserialize
        let data = vec![1, 0, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 116];
        assert_eq!(
            Foo {
                bar: 1,
                baz: Some(0),
                quz: "t".to_string(),
            },
            deserialize(&data).unwrap()
        );

        // Will panic because `quz` is missing, even though `baz` is tagged
        let data = vec![1, 0, 1, 0];
        assert_eq!(
            Foo {
                bar: 1,
                baz: None,
                quz: "".to_string(),
            },
            deserialize(&data).unwrap()
        );
    }
}
