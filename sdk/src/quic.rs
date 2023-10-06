//! Definitions related to Solana over QUIC.
use std::time::Duration;

pub const QUIC_PORT_OFFSET: u16 = 6;
// Empirically found max number of concurrent streams
// that seems to maximize TPS on GCE (higher values don't seem to
// give significant improvement or seem to impact stability)
pub const QUIC_MAX_UNSTAKED_CONCURRENT_STREAMS: usize = 128;
pub const QUIC_MIN_STAKED_CONCURRENT_STREAMS: usize = 128;

pub const QUIC_TOTAL_STAKED_CONCURRENT_STREAMS: usize = 100_000;

// Set the maximum concurrent stream numbers to avoid excessive streams.
// The value was lowered from 2048 to reduce contention of the limited
// receive_window among the streams which is observed in CI bench-tests with
// forwarded packets from staked nodes.
pub const QUIC_MAX_STAKED_CONCURRENT_STREAMS: usize = 512;

pub const QUIC_MAX_TIMEOUT: Duration = Duration::from_secs(2);
pub const QUIC_KEEP_ALIVE: Duration = Duration::from_secs(1);

// Based on commonly-used handshake timeouts for various TCP
// applications. Different applications vary, but most seem to
// be in the 30-60 second range
pub const QUIC_CONNECTION_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(60);

/// The receive window for QUIC connection from unstaked nodes is
/// set to this ratio times [`solana_sdk::packet::PACKET_DATA_SIZE`]
pub const QUIC_UNSTAKED_RECEIVE_WINDOW_RATIO: u64 = 1;

/// The receive window for QUIC connection from minimum staked nodes is
/// set to this ratio times [`solana_sdk::packet::PACKET_DATA_SIZE`]
pub const QUIC_MIN_STAKED_RECEIVE_WINDOW_RATIO: u64 = 2;

/// The receive window for QUIC connection from maximum staked nodes is
/// set to this ratio times [`solana_sdk::packet::PACKET_DATA_SIZE`]
pub const QUIC_MAX_STAKED_RECEIVE_WINDOW_RATIO: u64 = 10;
