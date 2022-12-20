//! Simple client that connects to a given UDP port with the QUIC protocol and provides
//! an interface for sending transactions which is restricted by the server's flow control.

use {
    crate::nonblocking::quic_client::{
        QuicClient, QuicLazyInitializedEndpoint, QuicTpuConnection as NonblockingQuicTpuConnection,
    },
    lazy_static::lazy_static,
    log::*,
    solana_sdk::transport::{Result as TransportResult, TransportError},
    solana_tpu_client::{
        connection_cache_stats::ConnectionCacheStats,
        nonblocking::tpu_connection::TpuConnection as NonblockingTpuConnection,
        tpu_connection::{ClientStats, TpuConnection},
    },
    std::{
        net::SocketAddr,
        sync::{atomic::Ordering, Arc, Condvar, Mutex, MutexGuard},
        time::Duration,
    },
    tokio::{runtime::Runtime, time::timeout},
};

pub mod temporary_pub {
    use super::*;

    pub const MAX_OUTSTANDING_TASK: u64 = 2000;
    pub const SEND_TRANSACTION_TIMEOUT_MS: u64 = 10000;

    /// A semaphore used for limiting the number of asynchronous tasks spawn to the
    /// runtime. Before spawnning a task, use acquire. After the task is done (be it
    /// succsess or failure), call release.
    pub struct AsyncTaskSemaphore {
        /// Keep the counter info about the usage
        counter: Mutex<u64>,
        /// Conditional variable for signaling when counter is decremented
        cond_var: Condvar,
        /// The maximum usage allowed by this semaphore.
        permits: u64,
    }

    impl AsyncTaskSemaphore {
        pub fn new(permits: u64) -> Self {
            Self {
                counter: Mutex::new(0),
                cond_var: Condvar::new(),
                permits,
            }
        }

        /// When returned, the lock has been locked and usage count has been
        /// incremented. When the returned MutexGuard is dropped the lock is dropped
        /// without decrementing the usage count.
        pub fn acquire(&self) -> MutexGuard<u64> {
            let mut count = self.counter.lock().unwrap();
            *count += 1;
            while *count > self.permits {
                count = self.cond_var.wait(count).unwrap();
            }
            count
        }

        /// Acquire the lock and decrement the usage count
        pub fn release(&self) {
            let mut count = self.counter.lock().unwrap();
            *count -= 1;
            self.cond_var.notify_one();
        }
    }

    lazy_static! {
        pub static ref ASYNC_TASK_SEMAPHORE: AsyncTaskSemaphore =
            AsyncTaskSemaphore::new(MAX_OUTSTANDING_TASK);
        pub static ref RUNTIME: Runtime = tokio::runtime::Builder::new_multi_thread()
            .thread_name("quic-client")
            .enable_all()
            .build()
            .unwrap();
    }

    pub async fn send_wire_transaction_async(
        connection: Arc<NonblockingQuicTpuConnection>,
        wire_transaction: Vec<u8>,
    ) -> TransportResult<()> {
        let result = timeout(
            Duration::from_millis(SEND_TRANSACTION_TIMEOUT_MS),
            connection.send_wire_transaction(wire_transaction),
        )
        .await;
        ASYNC_TASK_SEMAPHORE.release();
        handle_send_result(result, connection)
    }

    pub async fn send_wire_transaction_batch_async(
        connection: Arc<NonblockingQuicTpuConnection>,
        buffers: Vec<Vec<u8>>,
    ) -> TransportResult<()> {
        let time_out = SEND_TRANSACTION_TIMEOUT_MS * buffers.len() as u64;

        let result = timeout(
            Duration::from_millis(time_out),
            connection.send_wire_transaction_batch(&buffers),
        )
        .await;
        ASYNC_TASK_SEMAPHORE.release();
        handle_send_result(result, connection)
    }

    /// Check the send result and update stats if timedout. Returns the checked result.
    pub fn handle_send_result(
        result: Result<Result<(), TransportError>, tokio::time::error::Elapsed>,
        connection: Arc<NonblockingQuicTpuConnection>,
    ) -> Result<(), TransportError> {
        match result {
            Ok(result) => result,
            Err(_err) => {
                let client_stats = ClientStats::default();
                client_stats.send_timeout.fetch_add(1, Ordering::Relaxed);
                let stats = connection.connection_stats();
                stats.add_client_stats(&client_stats, 0, false);
                info!("Timedout sending transaction {:?}", connection.tpu_addr());
                Err(TransportError::Custom(
                    "Timedout sending transaction".to_string(),
                ))
            }
        }
    }
}
use temporary_pub::*;

pub struct QuicTpuConnection {
    pub inner: Arc<NonblockingQuicTpuConnection>,
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
