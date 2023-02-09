#[deprecated(
    since = "1.15.0",
    note = "Please use `solana_connection_cache::client_connection::ClientConnection` instead."
)]
pub use solana_connection_cache::client_connection::ClientConnection as TpuConnection;
pub use solana_connection_cache::client_connection::ClientStats;
