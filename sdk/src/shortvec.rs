use serde::de::Deserializer;
use serde::ser::Serializer;
use serde::Serialize;

// Use 1 to 9 bytes to store the length of the vector.
pub fn serialize<S: Serializer, T: Serialize>(
    elements: &[T],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeTuple;
    let mut seq = serializer.serialize_tuple(0)?;

    let mut rem_len = elements.len();
    loop {
        let mut elem = (rem_len & 0x7f) as u8;
        rem_len >>= 7;
        if rem_len == 0 {
            seq.serialize_element(&elem)?;
            break;
        } else {
            elem |= 0x80;
            seq.serialize_element(&elem)?;
        }
    }

    for element in elements {
        seq.serialize_element(element)?;
    }
    seq.end()
}

pub fn deserialize<'de, D, T: Serialize>(_deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
{
    unimplemented!();
}
