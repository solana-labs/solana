//! Simple client that connects to a given UDP port with the QUIC protocol and provides
//! an interface for sending data which is restricted by the server's flow control.

use {
    crate::nonblocking::quic_client::{
        QuicClient, QuicClientConnection as NonblockingQuicConnection, QuicLazyInitializedEndpoint,
    },
    lazy_static::lazy_static,
    log::*,
    solana_connection_cache::{
        client_connection::{ClientConnection, ClientStats},
        connection_cache_stats::ConnectionCacheStats,
        nonblocking::client_connection::ClientConnection as NonblockingClientConnection,
    },
    solana_sdk::transport::{Result as TransportResult, TransportError},
    std::{
        net::SocketAddr,
        sync::{atomic::Ordering, Arc, Condvar, Mutex, MutexGuard},
        time::Duration,
    },
    tokio::{runtime::Runtime, time::timeout},
};

pub const MAX_OUTSTANDING_TASK: u64 = 2000;
const SEND_DATA_TIMEOUT: Duration = Duration::from_secs(10);

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
    static ref ASYNC_TASK_SEMAPHORE: AsyncTaskSemaphore =
        AsyncTaskSemaphore::new(MAX_OUTSTANDING_TASK);
    static ref RUNTIME: Runtime = tokio::runtime::Builder::new_multi_thread()
        .thread_name("quic-client")
        .enable_all()
        .build()
        .unwrap();
}

async fn send_data_async(
    connection: Arc<NonblockingQuicConnection>,
    buffer: Vec<u8>,
) -> TransportResult<()> {
    let result = timeout(SEND_DATA_TIMEOUT, connection.send_data(&buffer)).await;
    ASYNC_TASK_SEMAPHORE.release();
    handle_send_result(result, connection)
}

async fn send_data_batch_async(
    connection: Arc<NonblockingQuicConnection>,
    buffers: Vec<Vec<u8>>,
) -> TransportResult<()> {
    let result = timeout(
        u32::try_from(buffers.len())
            .map(|size| SEND_DATA_TIMEOUT.saturating_mul(size))
            .unwrap_or(Duration::MAX),
        connection.send_data_batch(&buffers),
    )
    .await;
    ASYNC_TASK_SEMAPHORE.release();
    handle_send_result(result, connection)
}

/// Check the send result and update stats if timedout. Returns the checked result.
fn handle_send_result(
    result: Result<Result<(), TransportError>, tokio::time::error::Elapsed>,
    connection: Arc<NonblockingQuicConnection>,
) -> Result<(), TransportError> {
    match result {
        Ok(result) => result,
        Err(_err) => {
            let client_stats = ClientStats::default();
            client_stats.send_timeout.fetch_add(1, Ordering::Relaxed);
            let stats = connection.connection_stats();
            stats.add_client_stats(&client_stats, 0, false);
            info!("Timedout sending data {:?}", connection.server_addr());
            Err(TransportError::Custom("Timedout sending data".to_string()))
        }
    }
}

pub struct QuicClientConnection {
    pub inner: Arc<NonblockingQuicConnection>,
}

impl QuicClientConnection {
    pub fn new(
        endpoint: Arc<QuicLazyInitializedEndpoint>,
        server_addr: SocketAddr,
        connection_stats: Arc<ConnectionCacheStats>,
    ) -> Self {
        let inner = Arc::new(NonblockingQuicConnection::new(
            endpoint,
            server_addr,
            connection_stats,
        ));
        Self { inner }
    }

    pub fn new_with_client(
        client: Arc<QuicClient>,
        connection_stats: Arc<ConnectionCacheStats>,
    ) -> Self {
        let inner = Arc::new(NonblockingQuicConnection::new_with_client(
            client,
            connection_stats,
        ));
        Self { inner }
    }
}

impl ClientConnection for QuicClientConnection {
    fn server_addr(&self) -> &SocketAddr {
        self.inner.server_addr()
    }

    fn send_data_batch(&self, buffers: &[Vec<u8>]) -> TransportResult<()> {
        RUNTIME.block_on(self.inner.send_data_batch(buffers))?;
        Ok(())
    }

    fn send_data_async(&self, data: Vec<u8>) -> TransportResult<()> {
        let _lock = ASYNC_TASK_SEMAPHORE.acquire();
        let inner = self.inner.clone();

        let _handle = RUNTIME.spawn(send_data_async(inner, data));
        Ok(())
    }

    fn send_data_batch_async(&self, buffers: Vec<Vec<u8>>) -> TransportResult<()> {
        let _lock = ASYNC_TASK_SEMAPHORE.acquire();
        let inner = self.inner.clone();
        let _handle = RUNTIME.spawn(send_data_batch_async(inner, buffers));
        Ok(())
    }

    fn send_data(&self, buffer: &[u8]) -> TransportResult<()> {
        RUNTIME.block_on(self.inner.send_data(buffer))?;
        Ok(())
    }
}
