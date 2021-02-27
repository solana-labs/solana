use {
    jsonrpc_core::{MetaIoHandler, Metadata, Result},
    jsonrpc_core_client::{transports::ipc, RpcError},
    jsonrpc_derive::rpc,
    jsonrpc_ipc_server::{RequestContext, ServerBuilder},
    jsonrpc_server_utils::tokio,
    log::*,
    solana_core::validator::ValidatorExit,
    std::{
        net::SocketAddr,
        path::Path,
        sync::{Arc, RwLock},
        thread::Builder,
        time::SystemTime,
    },
};

#[derive(Clone)]
pub struct AdminRpcRequestMetadata {
    pub rpc_addr: Option<SocketAddr>,
    pub start_time: SystemTime,
    pub validator_exit: Arc<RwLock<ValidatorExit>>,
}
impl Metadata for AdminRpcRequestMetadata {}

#[rpc]
pub trait AdminRpc {
    type Metadata;

    #[rpc(meta, name = "exit")]
    fn exit(&self, meta: Self::Metadata) -> Result<()>;

    #[rpc(meta, name = "rpcAddress")]
    fn rpc_addr(&self, meta: Self::Metadata) -> Result<Option<SocketAddr>>;

    #[rpc(name = "setLogFilter")]
    fn set_log_filter(&self, filter: String) -> Result<()>;

    #[rpc(meta, name = "startTime")]
    fn start_time(&self, meta: Self::Metadata) -> Result<SystemTime>;
}

pub struct AdminRpcImpl;
impl AdminRpc for AdminRpcImpl {
    type Metadata = AdminRpcRequestMetadata;

    fn exit(&self, meta: Self::Metadata) -> Result<()> {
        info!("exit admin rpc request received");
        // Delay exit signal until this RPC request completes, otherwise the caller of `exit` might
        // receive a confusing error as the validator shuts down before a response is send back.
        tokio::spawn(async move {
            meta.validator_exit.write().unwrap().exit();
        });
        Ok(())
    }

    fn rpc_addr(&self, meta: Self::Metadata) -> Result<Option<SocketAddr>> {
        info!("rpc_addr admin rpc request received");
        Ok(meta.rpc_addr)
    }

    fn set_log_filter(&self, filter: String) -> Result<()> {
        info!("set_log_filter admin rpc request received");
        solana_logger::setup_with(&filter);
        Ok(())
    }

    fn start_time(&self, meta: Self::Metadata) -> Result<SystemTime> {
        info!("start_time admin rpc request received");
        Ok(meta.start_time)
    }
}

// Start the Admin RPC interface
pub fn run(ledger_path: &Path, metadata: AdminRpcRequestMetadata) {
    let admin_rpc_path = ledger_path.join("admin.rpc");

    let event_loop = tokio::runtime::Builder::new()
        .threaded_scheduler()
        .enable_all()
        .thread_name("sol-adminrpc-el")
        .build()
        .unwrap();

    Builder::new()
        .name("solana-adminrpc".to_string())
        .spawn(move || {
            let mut io = MetaIoHandler::default();
            io.extend_with(AdminRpcImpl.to_delegate());

            let validator_exit = metadata.validator_exit.clone();
            let server = ServerBuilder::with_meta_extractor(io, move |_req: &RequestContext| {
                metadata.clone()
            })
            .event_loop_executor(event_loop.handle().clone())
            .start(&format!("{}", admin_rpc_path.display()));

            match server {
                Err(err) => {
                    warn!("Unable to start admin rpc service: {:?}", err);
                }
                Ok(server) => {
                    let close_handle = server.close_handle();
                    validator_exit
                        .write()
                        .unwrap()
                        .register_exit(Box::new(move || {
                            close_handle.close();
                        }));

                    server.wait();
                }
            }
        })
        .unwrap();
}

// Connect to the Admin RPC interface
pub async fn connect(ledger_path: &Path) -> std::result::Result<gen_client::Client, RpcError> {
    let admin_rpc_path = ledger_path.join("admin.rpc");
    if !admin_rpc_path.exists() {
        Err(RpcError::Client(format!(
            "{} does not exist",
            admin_rpc_path.display()
        )))
    } else {
        ipc::connect::<_, gen_client::Client>(&format!("{}", admin_rpc_path.display())).await
    }
}

pub fn runtime() -> jsonrpc_server_utils::tokio::runtime::Runtime {
    jsonrpc_server_utils::tokio::runtime::Runtime::new().expect("new tokio runtime")
}
