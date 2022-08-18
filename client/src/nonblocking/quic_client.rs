//! Simple nonblocking client that connects to a given UDP port with the QUIC protocol
//! and provides an interface for sending transactions which is restricted by the
//! server's flow control.

pub use solana_tpu_client::nonblocking::quic_client::*;
