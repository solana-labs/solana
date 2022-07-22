pub const QUIC_PORT_OFFSET: u16 = 6;
// Empirically found max number of concurrent streams
// that seems to maximize TPS on GCE (higher values don't seem to
// give significant improvement or seem to impact stability)
pub const QUIC_MAX_UNSTAKED_CONCURRENT_STREAMS: usize = 128;
pub const QUIC_MIN_STAKED_CONCURRENT_STREAMS: usize = 128;

pub const QUIC_MAX_TIMEOUT_MS: u32 = 2_000;
pub const QUIC_KEEP_ALIVE_MS: u64 = 1_000;

// Based on commonly-used handshake timeouts for various TCP
// applications. Different applications vary, but most seem to
// be in the 30-60 second range
pub const QUIC_CONNECTION_HANDSHAKE_TIMEOUT_MS: u64 = 60_000;
