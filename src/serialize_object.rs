use bincode::{deserialize, serialize, ErrorKind};

pub fn deserialize_object(
    cur: &mut usize,
    snapshot: &[u8],
) -> core::result::Result<Vec<u8>, Box<ErrorKind>> {
    let len: u64 = deserialize(&snapshot[*cur..*cur + 8])?;
    let len = len as usize;
    *cur += 8;
    let v = snapshot[*cur..*cur + len].to_vec();
    *cur += len;
    Ok(v)
}

pub fn serialize_object(v: &mut Vec<u8>, new: Vec<u8>) {
    let len = new.len() as u64;
    v.extend(serialize(&len).unwrap());
    v.extend(new);
}
