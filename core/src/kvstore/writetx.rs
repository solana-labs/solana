use crate::kvstore::error::Result;
use crate::kvstore::sstable::Key;

#[derive(Debug)]
pub struct WriteTx<'a> {
    _dummy: &'a mut (),
}

impl<'a> WriteTx<'a> {
    pub fn put(&mut self, _key: &Key, _data: &[u8]) -> Result<()> {
        unimplemented!()
    }

    pub fn delete(&mut self, _key: &Key) -> Result<()> {
        unimplemented!()
    }
}
