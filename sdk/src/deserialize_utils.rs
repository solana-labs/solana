use serde::{Deserialize, Deserializer};

pub fn default_on_eof<'de, T, D>(d: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    let result = T::deserialize(d);
    match result {
        Err(err) if err.to_string() == "io error: unexpected end of file" => Ok(T::default()),
        result => result,
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use bincode::deserialize;

    #[test]
    fn test_default_on_eof() {
        #[derive(Serialize, Deserialize, Debug, PartialEq)]
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
}
