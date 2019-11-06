use crate::cuda_runtime::PinnedVec;
use crate::packet::PacketsRecycler;
use crate::recycler::Recycler;
use crate::sigverify::TxOffset;

pub const PACKETS_CAPACITY: usize = 32;

#[derive(Default, Clone)]
pub struct RecyclerCache {
    recycler_packets: PacketsRecycler,
    recycler_offsets: Recycler<TxOffset>,
    recycler_buffer: Recycler<PinnedVec<u8>>,
}

impl RecyclerCache {
    pub fn warmed() -> Self {
        Self {
            recycler_packets: Recycler::warmed(1000, PACKETS_CAPACITY),
            recycler_offsets: Recycler::warmed(50, 4096),
            recycler_buffer: Recycler::warmed(50, 4096),
        }
    }
    pub fn packets(&self) -> &PacketsRecycler {
        &self.recycler_packets
    }
    pub fn offsets(&self) -> &Recycler<TxOffset> {
        &self.recycler_offsets
    }
    pub fn buffer(&self) -> &Recycler<PinnedVec<u8>> {
        &self.recycler_buffer
    }
}
