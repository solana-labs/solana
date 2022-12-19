//! Simple client that connects to a given UDP port with the QUIC protocol and provides
//! an interface for sending transactions which is restricted by the server's flow control.

pub use solana_quic_client::quic_client::QuicTpuConnection;
use {
    crate::{
        nonblocking::tpu_connection::TpuConnection as NonblockingTpuConnection,
        tpu_connection::TpuConnection,
    },
    solana_quic_client::quic_client::temporary_pub::*,
    solana_sdk::transport::Result as TransportResult,
    std::net::SocketAddr,
};

impl TpuConnection for QuicTpuConnection {
    fn tpu_addr(&self) -> &SocketAddr {
        self.inner.tpu_addr()
    }

    fn send_wire_transaction_batch<T>(&self, buffers: &[T]) -> TransportResult<()>
    where
        T: AsRef<[u8]> + Send + Sync,
    {
        RUNTIME.block_on(self.inner.send_wire_transaction_batch(buffers))?;
        Ok(())
    }

    fn send_wire_transaction_async(&self, wire_transaction: Vec<u8>) -> TransportResult<()> {
        let _lock = ASYNC_TASK_SEMAPHORE.acquire();
        let inner = self.inner.clone();

        let _handle = RUNTIME
            .spawn(async move { send_wire_transaction_async(inner, wire_transaction).await });
        Ok(())
    }

    fn send_wire_transaction_batch_async(&self, buffers: Vec<Vec<u8>>) -> TransportResult<()> {
        let _lock = ASYNC_TASK_SEMAPHORE.acquire();
        let inner = self.inner.clone();
        let _handle =
            RUNTIME.spawn(async move { send_wire_transaction_batch_async(inner, buffers).await });
        Ok(())
    }
}
