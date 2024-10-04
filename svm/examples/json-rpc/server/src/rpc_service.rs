use {
    crate::rpc_process::{rpc::*, *},
    crossbeam_channel::unbounded,
    jsonrpc_core::MetaIoHandler,
    jsonrpc_http_server::{
        hyper, AccessControlAllowOrigin, CloseHandle, DomainsValidation, ServerBuilder,
    },
    log::*,
    solana_perf::thread::renice_this_thread,
    solana_sdk::exit::Exit,
    std::{
        net::SocketAddr,
        sync::{atomic::AtomicBool, Arc, RwLock},
        thread::{self, Builder, JoinHandle},
    },
};

pub struct JsonRpcService {
    thread_hdl: JoinHandle<()>,
    close_handle: Option<CloseHandle>,
}

impl JsonRpcService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        rpc_addr: SocketAddr,
        config: JsonRpcConfig,
        validator_exit: Arc<RwLock<Exit>>,
        _exit: Arc<AtomicBool>,
    ) -> Result<Self, String> {
        info!("rpc bound to {:?}", rpc_addr);
        info!("rpc configuration: {:?}", config);
        let rpc_threads = 1.max(config.rpc_threads);
        let rpc_niceness_adj = config.rpc_niceness_adj;

        let runtime = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(rpc_threads)
                .on_thread_start(move || renice_this_thread(rpc_niceness_adj).unwrap())
                .thread_name("solRpcEl")
                .enable_all()
                .build()
                .expect("Runtime"),
        );

        let max_request_body_size = config
            .max_request_body_size
            .unwrap_or(MAX_REQUEST_BODY_SIZE);
        let request_processor = JsonRpcRequestProcessor::new(config, validator_exit.clone());
        let (close_handle_sender, close_handle_receiver) = unbounded();
        let thread_hdl = Builder::new()
            .name("solJsonRpcSvc".to_string())
            .spawn(move || {
                renice_this_thread(rpc_niceness_adj).unwrap();
                let mut io = MetaIoHandler::default();
                io.extend_with(rpc::RpcImpl.to_delegate());
                let server = ServerBuilder::with_meta_extractor(
                    io,
                    move |_req: &hyper::Request<hyper::Body>| request_processor.clone(),
                )
                .event_loop_executor(runtime.handle().clone())
                .threads(1)
                .cors(DomainsValidation::AllowOnly(vec![
                    AccessControlAllowOrigin::Any,
                ]))
                .cors_max_age(86400)
                .max_request_body_size(max_request_body_size)
                .start_http(&rpc_addr);

                if let Err(e) = server {
                    warn!(
                        "JSON RPC service unavailable error: {:?}. \n\
                           Also, check that port {} is not already in use by another application",
                        e,
                        rpc_addr.port()
                    );
                    close_handle_sender.send(Err(e.to_string())).unwrap();
                    return;
                }

                let server = server.unwrap();
                close_handle_sender.send(Ok(server.close_handle())).unwrap();
                server.wait();
            })
            .unwrap();

        let close_handle = close_handle_receiver.recv().unwrap()?;
        let close_handle_ = close_handle.clone();
        validator_exit
            .write()
            .unwrap()
            .register_exit(Box::new(move || {
                close_handle_.close();
            }));
        Ok(Self {
            thread_hdl,
            close_handle: Some(close_handle),
        })
    }

    pub fn exit(&mut self) {
        if let Some(c) = self.close_handle.take() {
            c.close()
        }
    }

    pub fn join(mut self) -> thread::Result<()> {
        self.exit();
        self.thread_hdl.join()
    }
}
