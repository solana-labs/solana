use crate::cuda_runtime::PinnedVec;
use crate::recycler::Recycler;
use crate::sigverify::TxOffset;

#[derive(Default, Clone)]
pub struct RecyclerCache {
    recycler_offsets: Recycler<TxOffset>,
    recycler_buffer: Recycler<PinnedVec<u8>>,
}

impl RecyclerCache {
    pub fn warmed() -> Self {
        Self {
            recycler_offsets: Recycler::warmed(50, 4096),
            recycler_buffer: Recycler::warmed(50, 4096),
        }
    }
    pub fn offsets(&self) -> &Recycler<TxOffset> {
        &self.recycler_offsets
    }
    pub fn buffer(&self) -> &Recycler<PinnedVec<u8>> {
        &self.recycler_buffer
    }
}
