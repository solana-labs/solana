use crate::cuda_runtime::PinnedVec;
use crate::recycler::Recycler;
use crate::sigverify::TxOffset;

#[derive(Clone)]
pub struct RecyclerCache {
    recycler_offsets: Recycler<TxOffset>,
    recycler_buffer: Recycler<PinnedVec<u8>>,
}

impl RecyclerCache {
    pub fn new(offsets_shrink_name: &'static str, buffer_shrink_name: &'static str) -> Self {
        Self {
            recycler_offsets: Recycler::new_without_limit(offsets_shrink_name),
            recycler_buffer: Recycler::new_without_limit(buffer_shrink_name),
        }
    }

    pub fn warmed(offsets_shrink_name: &'static str, buffer_shrink_name: &'static str) -> Self {
        Self {
            recycler_offsets: Recycler::warmed(50, 4096, None, offsets_shrink_name),
            recycler_buffer: Recycler::warmed(50, 4096, None, buffer_shrink_name),
        }
    }
    pub fn offsets(&self) -> &Recycler<TxOffset> {
        &self.recycler_offsets
    }
    pub fn buffer(&self) -> &Recycler<PinnedVec<u8>> {
        &self.recycler_buffer
    }
}
