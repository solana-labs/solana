//! The `net_utils` module assists with networking
#![allow(clippy::integer_arithmetic)]

mod ip_echo_server;
use ip_echo_server::IpEchoServerMessage;
pub use ip_echo_server::{ip_echo_server, IpEchoServer, MAX_PORT_COUNT_PER_MESSAGE};

mod socket_like;
pub use socket_like::*;

mod network;
pub use network::*;

mod std_network;
pub use std_network::*;

mod helpers;
pub use helpers::*;

pub type PortRange = (u16, u16);


pub mod streamer;
