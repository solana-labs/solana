//! Simple client that connects to a given UDP port with the QUIC protocol and provides
//! an interface for sending transactions which is restricted by the server's flow control.

use {
    crate::{
        connection_cache_stats::ConnectionCacheStats,
        nonblocking::{
            quic_client::{
                QuicClient, QuicLazyInitializedEndpoint,
                QuicTpuConnection as NonblockingQuicTpuConnection,
            },
            tpu_connection::TpuConnection as NonblockingTpuConnection,
        },
        tpu_connection::TpuConnection,
    },
    lazy_static::lazy_static,
    solana_sdk::transport::Result as TransportResult,
    std::{
        net::SocketAddr,
        sync::{Arc, Condvar, Mutex, MutexGuard},
    },
    tokio::runtime::Runtime,
};

const MAX_OUTSTANDING_TASK: u64 = 2000;

/// A semaphore used for limiting the number of asynchronous tasks spawn to the
/// runtime. Before spawnning a task, use acquire. After the task is done (be it
/// succsess or failure), call release.
struct AsyncTaskSemaphore {
    /// Keep the counter info about the usage
    counter: Mutex<u64>,
    /// Conditional variable for signaling when counter is decremented
    cond_var: Condvar,
    /// The maximum usage allowed by this semaphore.
    permits: u64,
}

impl AsyncTaskSemaphore {
    fn new(permits: u64) -> Self {
        Self {
            counter: Mutex::new(0),
            cond_var: Condvar::new(),
            permits,
        }
    }

    /// When returned, the lock has been locked and usage count has been
    /// incremented. When the returned MutexGuard is dropped the lock is dropped
    /// without decrementing the usage count.
    fn acquire(&self) -> MutexGuard<u64> {
        let mut count = self.counter.lock().unwrap();
        *count += 1;
        while *count > self.permits {
            count = self.cond_var.wait(count).unwrap();
        }
        count
    }

    /// Acquire the lock and decrement the usage count
    fn release(&self) {
        let mut count = self.counter.lock().unwrap();
        *count -= 1;
        self.cond_var.notify_one();
    }
}

lazy_static! {
    static ref ASYNC_TASK_SEMAPHORE: AsyncTaskSemaphore =
        AsyncTaskSemaphore::new(MAX_OUTSTANDING_TASK);
    static ref RUNTIME: Runtime = tokio::runtime::Builder::new_multi_thread()
        .thread_name("quic-client")
        .enable_all()
        .build()
        .unwrap();
}

pub struct QuicTpuConnection {
    inner: Arc<NonblockingQuicTpuConnection>,
}
impl QuicTpuConnection {
    pub fn new(
        endpoint: Arc<QuicLazyInitializedEndpoint>,
        tpu_addr: SocketAddr,
        connection_stats: Arc<ConnectionCacheStats>,
    ) -> Self {
        let inner = Arc::new(NonblockingQuicTpuConnection::new(
            endpoint,
            tpu_addr,
            connection_stats,
        ));
        Self { inner }
    }

    pub fn new_with_client(
        client: Arc<QuicClient>,
        connection_stats: Arc<ConnectionCacheStats>,
    ) -> Self {
        let inner = Arc::new(NonblockingQuicTpuConnection::new_with_client(
            client,
            connection_stats,
        ));
        Self { inner }
    }
}

async fn send_wire_transaction_async(
    connection: Arc<NonblockingQuicTpuConnection>,
    wire_transaction: Vec<u8>,
) -> TransportResult<()> {
    let result = connection.send_wire_transaction(wire_transaction).await;
    ASYNC_TASK_SEMAPHORE.release();
    result
}

async fn send_wire_transaction_batch_async(
    connection: Arc<NonblockingQuicTpuConnection>,
    buffers: Vec<Vec<u8>>,
) -> TransportResult<()> {
    let result = connection.send_wire_transaction_batch(&buffers).await;
    ASYNC_TASK_SEMAPHORE.release();
    result
}

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

        let _ = RUNTIME
            .spawn(async move { send_wire_transaction_async(inner, wire_transaction).await });
        Ok(())
    }

    fn send_wire_transaction_batch_async(&self, buffers: Vec<Vec<u8>>) -> TransportResult<()> {
        let _lock = ASYNC_TASK_SEMAPHORE.acquire();
        let inner = self.inner.clone();
        let _ =
            RUNTIME.spawn(async move { send_wire_transaction_batch_async(inner, buffers).await });
        Ok(())
    }
}
