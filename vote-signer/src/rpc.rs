//! The `rpc` module implements the Vote signing service RPC interface.

use bs58;
use jsonrpc_core::*;
use jsonrpc_http_server::*;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use std::mem;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, sleep, Builder, JoinHandle};
use std::time::Duration;

pub struct VoteSignerRpcService {
    thread_hdl: JoinHandle<()>,
    exit: Arc<AtomicBool>,
}

impl VoteSignerRpcService {
    pub fn new(rpc_addr: SocketAddr) -> Self {
        let request_processor = VoteSignRequestProcessor::new();
        let exit = Arc::new(AtomicBool::new(false));
        let exit_ = exit.clone();
        let thread_hdl = Builder::new()
            .name("solana-vote-signer-jsonrpc".to_string())
            .spawn(move || {
                let mut io = MetaIoHandler::default();
                let rpc = VoteSignerRpcImpl;
                io.extend_with(rpc.to_delegate());

                let server =
                    ServerBuilder::with_meta_extractor(io, move |_req: &hyper::Request<hyper::Body>| Meta {
                        request_processor: request_processor.clone(),
                        rpc_addr,
                    }).threads(4)
                        .cors(DomainsValidation::AllowOnly(vec![
                            AccessControlAllowOrigin::Any,
                        ]))
                        .start_http(&rpc_addr);
                if server.is_err() {
                    warn!("JSON RPC service unavailable: unable to bind to RPC port {}. \nMake sure this port is not already in use by another application", rpc_addr.port());
                    return;
                }
                while !exit_.load(Ordering::Relaxed) {
                    sleep(Duration::from_millis(100));
                }
                server.unwrap().close();
                ()
            })
            .unwrap();
        VoteSignerRpcService { thread_hdl, exit }
    }

    pub fn exit(&self) {
        self.exit.store(true, Ordering::Relaxed);
    }

    pub fn close(self) -> thread::Result<()> {
        self.exit();
        self.join()
    }

    pub fn join(self) -> thread::Result<()> {
        self.thread_hdl.join()
    }
}

#[derive(Clone)]
pub struct Meta {
    pub request_processor: VoteSignRequestProcessor,
    pub rpc_addr: SocketAddr,
}
impl Metadata for Meta {}

build_rpc_trait! {
    pub trait VoteSignerRpc {
        type Metadata;

        #[rpc(meta, name = "registerNode")]
        fn register(&self, Self::Metadata, String) -> Result<Pubkey>;

        #[rpc(meta, name = "signVote")]
        fn sign(&self, Self::Metadata, String) -> Result<Signature>;

        #[rpc(meta, name = "deregisterNode")]
        fn deregister(&self, Self::Metadata, String) -> Result<()>;
    }
}

pub struct VoteSignerRpcImpl;
impl VoteSignerRpc for VoteSignerRpcImpl {
    type Metadata = Meta;

    fn register(&self, meta: Self::Metadata, id: String) -> Result<Pubkey> {
        info!("register rpc request received: {:?}", id);
        let pubkey = get_pubkey(id)?;
        meta.request_processor.register(pubkey)
    }

    fn sign(&self, meta: Self::Metadata, id: String) -> Result<Signature> {
        info!("sign rpc request received: {:?}", id);
        let pubkey = get_pubkey(id)?;
        meta.request_processor.sign(pubkey)
    }

    fn deregister(&self, meta: Self::Metadata, id: String) -> Result<()> {
        info!("deregister rpc request received: {:?}", id);
        let pubkey = get_pubkey(id)?;
        meta.request_processor.deregister(pubkey)
    }
}

#[derive(Clone)]
pub struct VoteSignRequestProcessor {}
impl VoteSignRequestProcessor {
    /// Create a new request processor that wraps the given Bank.
    pub fn new() -> Self {
        VoteSignRequestProcessor {}
    }

    /// Process JSON-RPC request items sent via JSON-RPC.
    pub fn register(&self, pubkey: Pubkey) -> Result<Pubkey> {
        Ok(pubkey)
    }
    pub fn sign(&self, _pubkey: Pubkey) -> Result<Signature> {
        let signature = [0u8; 16];
        Ok(Signature::new(&signature))
    }
    pub fn deregister(&self, _pubkey: Pubkey) -> Result<()> {
        Ok(())
    }
}

fn get_pubkey(input: String) -> Result<Pubkey> {
    let pubkey_vec = bs58::decode(input).into_vec().map_err(|err| {
        info!("get_pubkey: invalid input: {:?}", err);
        Error::invalid_request()
    })?;
    if pubkey_vec.len() != mem::size_of::<Pubkey>() {
        info!(
            "get_pubkey: invalid pubkey_vec length: {}",
            pubkey_vec.len()
        );
        Err(Error::invalid_request())
    } else {
        Ok(Pubkey::new(&pubkey_vec))
    }
}

#[cfg(test)]
mod tests {}
