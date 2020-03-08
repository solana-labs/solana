use solana_sdk::clock::Slot;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
enum CompressionType {
    Uncompressed,
    GZip,
    BZip2,
}

impl Default for CompressionType {
    fn default() -> Self {
        Self::Uncompressed
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub(crate) struct EpochIncompleteSlots {
    first: Slot,
    compression: CompressionType,
    compressed_list: Vec<u8>,
}
