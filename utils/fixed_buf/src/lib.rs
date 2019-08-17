//! ser- and deser-ialize-able, fixed-size bufs

#[macro_export]
macro_rules! fixed_buf {
    ($name:ident, $size:expr) => {
        fixed_buf!($name, $size, 12);
    };
    ($name:ident, $size:expr, $max_print: expr) => {
        pub struct $name {
            _inner: [u8; $size],
        }

        impl Default for $name {
            fn default() -> Self {
                $name { _inner: [0; $size] }
            }
        }

        impl PartialEq for $name {
            fn eq(&self, other: &Self) -> bool {
                self.iter().zip(other.iter()).all(|(me, other)| me == other)
            }
        }
        impl Eq for $name {}

        impl std::ops::Deref for $name {
            type Target = [u8; $size];
            fn deref(&self) -> &Self::Target {
                &self._inner
            }
        }

        impl std::ops::DerefMut for $name {
            fn deref_mut(&mut self) -> &mut Self::Target {
                &mut self._inner
            }
        }

        impl std::fmt::Debug for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "[{}", self[0])?;
                for i in 1..self.len().min($max_print) {
                    write!(f, ", {}", self[i])?;
                }
                if $max_print < self.len() {
                    write!(f, ", ...")?;
                }
                write!(f, "]")
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct Visitor;
                impl<'de> serde::de::Visitor<'de> for Visitor {
                    type Value = $name;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                        formatter.write_str(stringify!($name))
                    }
                    fn visit_bytes<E>(self, bytes: &[u8]) -> Result<Self::Value, E>
                    where
                        E: serde::de::Error,
                    {
                        if bytes.len() != $size {
                            Err(serde::de::Error::invalid_length(
                                bytes.len(),
                                &stringify!($size),
                            ))?
                        }
                        let mut buf = $name::default();
                        buf.copy_from_slice(bytes);

                        Ok(buf)
                    }
                }
                deserializer.deserialize_bytes(Visitor)
            }
        }

        impl serde::ser::Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
            where
                S: serde::ser::Serializer,
            {
                serializer.serialize_bytes(&self._inner)
            }
        }
    };
}

#[cfg(test)]
mod test {
    use serde_derive::{Deserialize, Serialize};

    fixed_buf!(BufMath, 33);

    #[derive(Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
    pub struct Foo {
        pub foo: u64,
        pub buf: BufMath,
    }

    fixed_buf!(Buf34, 34, 35); // print even more than is present!

    #[derive(Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
    pub struct Foo34 {
        pub foo: u64,
        pub buf: Buf34,
    }

    #[test]
    fn test() {
        let mut foo = Foo {
            foo: 33,
            ..Foo::default()
        };
        foo.buf[1] = 127;
        let ser = bincode::serialize(&foo).unwrap();
        assert_eq!(bincode::deserialize::<Foo>(&ser).unwrap(), foo);

        assert_eq!(
            format!("{:?}", foo),
            "Foo { foo: 33, buf: [0, 127, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, ...] }"
        );

        let mut foo = Foo34 {
            foo: 34,
            ..Foo34::default()
        };
        foo.buf[1] = 128;
        let ser = bincode::serialize(&foo).unwrap();
        assert_eq!(bincode::deserialize::<Foo34>(&ser).unwrap(), foo);

        assert!(bincode::deserialize::<Foo>(&ser).is_err());

        assert_eq!(format!("{:?}", foo), "Foo34 { foo: 34, buf: [0, 128, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0] }");
    }
}
