//! This module defines [`SendTransactionStats`] which is used to collect per IP
//! statistics about relevant network errors.

use {
    super::QuicError,
    quinn::{ConnectError, ConnectionError, WriteError},
    std::{
        collections::HashMap,
        fmt,
        net::IpAddr,
        sync::{
            atomic::{AtomicU64, Ordering},
            Arc,
        },
    },
};

/// [`SendTransactionStats`] aggregates counters related to sending transactions.
#[derive(Debug, Default)]
pub struct SendTransactionStats {
    pub successfully_sent: AtomicU64,
    pub connect_error_cids_exhausted: AtomicU64,
    pub connect_error_invalid_remote_address: AtomicU64,
    pub connect_error_other: AtomicU64,
    pub connection_error_application_closed: AtomicU64,
    pub connection_error_cids_exhausted: AtomicU64,
    pub connection_error_connection_closed: AtomicU64,
    pub connection_error_locally_closed: AtomicU64,
    pub connection_error_reset: AtomicU64,
    pub connection_error_timed_out: AtomicU64,
    pub connection_error_transport_error: AtomicU64,
    pub connection_error_version_mismatch: AtomicU64,
    pub write_error_closed_stream: AtomicU64,
    pub write_error_connection_lost: AtomicU64,
    pub write_error_stopped: AtomicU64,
    pub write_error_zero_rtt_rejected: AtomicU64,
}

#[allow(clippy::arithmetic_side_effects)]
pub fn record_error(err: QuicError, stats: &SendTransactionStats) {
    match err {
        QuicError::Connect(ConnectError::EndpointStopping) => {
            stats.connect_error_other.fetch_add(1, Ordering::Relaxed);
        }
        QuicError::Connect(ConnectError::CidsExhausted) => {
            stats
                .connect_error_cids_exhausted
                .fetch_add(1, Ordering::Relaxed);
        }
        QuicError::Connect(ConnectError::InvalidServerName(_)) => {
            stats.connect_error_other.fetch_add(1, Ordering::Relaxed);
        }
        QuicError::Connect(ConnectError::InvalidRemoteAddress(_)) => {
            stats
                .connect_error_invalid_remote_address
                .fetch_add(1, Ordering::Relaxed);
        }
        QuicError::Connect(ConnectError::NoDefaultClientConfig) => {
            stats.connect_error_other.fetch_add(1, Ordering::Relaxed);
        }
        QuicError::Connect(ConnectError::UnsupportedVersion) => {
            stats.connect_error_other.fetch_add(1, Ordering::Relaxed);
        }
        QuicError::Connection(ConnectionError::VersionMismatch) => {
            stats
                .connection_error_version_mismatch
                .fetch_add(1, Ordering::Relaxed);
        }
        QuicError::Connection(ConnectionError::TransportError(_)) => {
            stats
                .connection_error_transport_error
                .fetch_add(1, Ordering::Relaxed);
        }
        QuicError::Connection(ConnectionError::ConnectionClosed(_)) => {
            stats
                .connection_error_connection_closed
                .fetch_add(1, Ordering::Relaxed);
        }
        QuicError::Connection(ConnectionError::ApplicationClosed(_)) => {
            stats
                .connection_error_application_closed
                .fetch_add(1, Ordering::Relaxed);
        }
        QuicError::Connection(ConnectionError::Reset) => {
            stats.connection_error_reset.fetch_add(1, Ordering::Relaxed);
        }
        QuicError::Connection(ConnectionError::TimedOut) => {
            stats
                .connection_error_timed_out
                .fetch_add(1, Ordering::Relaxed);
        }
        QuicError::Connection(ConnectionError::LocallyClosed) => {
            stats
                .connection_error_locally_closed
                .fetch_add(1, Ordering::Relaxed);
        }
        QuicError::Connection(ConnectionError::CidsExhausted) => {
            stats
                .connection_error_cids_exhausted
                .fetch_add(1, Ordering::Relaxed);
        }
        QuicError::StreamWrite(WriteError::Stopped(_)) => {
            stats.write_error_stopped.fetch_add(1, Ordering::Relaxed);
        }
        QuicError::StreamWrite(WriteError::ConnectionLost(_)) => {
            stats
                .write_error_connection_lost
                .fetch_add(1, Ordering::Relaxed);
        }
        QuicError::StreamWrite(WriteError::ClosedStream) => {
            stats
                .write_error_closed_stream
                .fetch_add(1, Ordering::Relaxed);
        }
        QuicError::StreamWrite(WriteError::ZeroRttRejected) => {
            stats
                .write_error_zero_rtt_rejected
                .fetch_add(1, Ordering::Relaxed);
        }
        // Endpoint is created on the scheduler level and handled separately
        // No counters are used for this case.
        QuicError::Endpoint(_) => (),
    }
}

pub type SendTransactionStatsPerAddr = HashMap<IpAddr, Arc<SendTransactionStats>>;

macro_rules! display_send_transaction_stats_body {
    ($self:ident, $f:ident, $($field:ident),* $(,)?) => {
        write!(
            $f,
            concat!(
                "SendTransactionStats:\n",
                $(
                    "\x20   ", stringify!($field), ": {},\n",
                )*
            ),
            $($self.$field.load(Ordering::Relaxed)),*
        )
    };
}

impl fmt::Display for SendTransactionStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        display_send_transaction_stats_body!(
            self,
            f,
            successfully_sent,
            connect_error_cids_exhausted,
            connect_error_invalid_remote_address,
            connect_error_other,
            connection_error_application_closed,
            connection_error_cids_exhausted,
            connection_error_connection_closed,
            connection_error_locally_closed,
            connection_error_reset,
            connection_error_timed_out,
            connection_error_transport_error,
            connection_error_version_mismatch,
            write_error_closed_stream,
            write_error_connection_lost,
            write_error_stopped,
            write_error_zero_rtt_rejected,
        )
    }
}

/// For tests it is useful to be have PartialEq but we cannot have it on top of
/// atomics. This macro creates a structure with the same attributes but of type
/// u64.
macro_rules! define_non_atomic_struct_for {
    ($name:ident, $atomic_name:ident, {$($field:ident),* $(,)?}) => {
        #[derive(Debug, Default, PartialEq)]
        pub struct $name {
            $(pub $field: u64),*
        }

        impl $atomic_name {
            pub fn to_non_atomic(&self) -> $name {
                $name {
                    $($field: self.$field.load(Ordering::Relaxed)),*
                }
            }
        }
    };
}

// Define the non-atomic struct and the `to_non_atomic` conversion method
define_non_atomic_struct_for!(
    SendTransactionStatsNonAtomic,
    SendTransactionStats,
    {
        successfully_sent,
        connect_error_cids_exhausted,
        connect_error_invalid_remote_address,
        connect_error_other,
        connection_error_application_closed,
        connection_error_cids_exhausted,
        connection_error_connection_closed,
        connection_error_locally_closed,
        connection_error_reset,
        connection_error_timed_out,
        connection_error_transport_error,
        connection_error_version_mismatch,
        write_error_closed_stream,
        write_error_connection_lost,
        write_error_stopped,
        write_error_zero_rtt_rejected
    }
);
