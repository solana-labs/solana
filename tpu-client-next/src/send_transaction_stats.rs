//! This module defines [`SendTransactionStats`] which is used to collect per IP
//! statistics about relevant network errors.

use {
    super::QuicError,
    quinn::{ConnectError, ConnectionError, WriteError},
    std::{collections::HashMap, fmt, net::IpAddr},
};

/// [`SendTransactionStats`] aggregates counters related to sending transactions.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct SendTransactionStats {
    pub successfully_sent: u64,
    pub connect_error_cids_exhausted: u64,
    pub connect_error_invalid_remote_address: u64,
    pub connect_error_other: u64,
    pub connection_error_application_closed: u64,
    pub connection_error_cids_exhausted: u64,
    pub connection_error_connection_closed: u64,
    pub connection_error_locally_closed: u64,
    pub connection_error_reset: u64,
    pub connection_error_timed_out: u64,
    pub connection_error_transport_error: u64,
    pub connection_error_version_mismatch: u64,
    pub write_error_closed_stream: u64,
    pub write_error_connection_lost: u64,
    pub write_error_stopped: u64,
    pub write_error_zero_rtt_rejected: u64,
}

#[allow(clippy::arithmetic_side_effects)]
pub fn record_error(err: QuicError, stats: &mut SendTransactionStats) {
    match err {
        QuicError::Connect(ConnectError::EndpointStopping) => {
            stats.connect_error_other += 1;
        }
        QuicError::Connect(ConnectError::CidsExhausted) => {
            stats.connect_error_cids_exhausted += 1;
        }
        QuicError::Connect(ConnectError::InvalidServerName(_)) => {
            stats.connect_error_other += 1;
        }
        QuicError::Connect(ConnectError::InvalidRemoteAddress(_)) => {
            stats.connect_error_invalid_remote_address += 1;
        }
        QuicError::Connect(ConnectError::NoDefaultClientConfig) => {
            stats.connect_error_other += 1;
        }
        QuicError::Connect(ConnectError::UnsupportedVersion) => {
            stats.connect_error_other += 1;
        }
        QuicError::Connection(ConnectionError::VersionMismatch) => {
            stats.connection_error_version_mismatch += 1;
        }
        QuicError::Connection(ConnectionError::TransportError(_)) => {
            stats.connection_error_transport_error += 1;
        }
        QuicError::Connection(ConnectionError::ConnectionClosed(_)) => {
            stats.connection_error_connection_closed += 1;
        }
        QuicError::Connection(ConnectionError::ApplicationClosed(_)) => {
            stats.connection_error_application_closed += 1;
        }
        QuicError::Connection(ConnectionError::Reset) => {
            stats.connection_error_reset += 1;
        }
        QuicError::Connection(ConnectionError::TimedOut) => {
            stats.connection_error_timed_out += 1;
        }
        QuicError::Connection(ConnectionError::LocallyClosed) => {
            stats.connection_error_locally_closed += 1;
        }
        QuicError::Connection(ConnectionError::CidsExhausted) => {
            stats.connection_error_cids_exhausted += 1;
        }
        QuicError::StreamWrite(WriteError::Stopped(_)) => {
            stats.write_error_stopped += 1;
        }
        QuicError::StreamWrite(WriteError::ConnectionLost(_)) => {
            stats.write_error_connection_lost += 1;
        }
        QuicError::StreamWrite(WriteError::ClosedStream) => {
            stats.write_error_closed_stream += 1;
        }
        QuicError::StreamWrite(WriteError::ZeroRttRejected) => {
            stats.write_error_zero_rtt_rejected += 1;
        }
        // Endpoint is created on the scheduler level and handled separately
        // No counters are used for this case.
        QuicError::Endpoint(_) => (),
    }
}

pub type SendTransactionStatsPerAddr = HashMap<IpAddr, SendTransactionStats>;

macro_rules! add_fields {
    ($self:ident += $other:ident for: $( $field:ident ),* $(,)? ) => {
        $(
            $self.$field = $self.$field.saturating_add($other.$field);
        )*
    };
}

impl SendTransactionStats {
    pub fn add(&mut self, other: &SendTransactionStats) {
        add_fields!(
            self += other for:
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
        );
    }
}

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
            $($self.$field),*
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
