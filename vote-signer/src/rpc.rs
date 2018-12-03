//! The `rpc` module implements the Vote signing service RPC interface.

use jsonrpc_core::*;
use jsonrpc_http_server::*;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil, Signature};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread::{self, sleep, Builder, JoinHandle};
use std::time::Duration;

pub struct VoteSignerRpcService {
    thread_hdl: JoinHandle<()>,
    exit: Arc<AtomicBool>,
}

impl VoteSignerRpcService {
    pub fn new(rpc_addr: SocketAddr) -> Self {
        let request_processor = VoteSignRequestProcessor::default();
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
}
impl Metadata for Meta {}

build_rpc_trait! {
    pub trait VoteSignerRpc {
        type Metadata;

        #[rpc(meta, name = "registerNode")]
        fn register(&self, Self::Metadata, Pubkey, Signature, Vec<u8>) -> Result<Pubkey>;

        #[rpc(meta, name = "signVote")]
        fn sign(&self, Self::Metadata, Pubkey, Signature, Vec<u8>) -> Result<Signature>;

        #[rpc(meta, name = "deregisterNode")]
        fn deregister(&self, Self::Metadata, Pubkey, Signature, Vec<u8>) -> Result<()>;
    }
}

pub struct VoteSignerRpcImpl;
impl VoteSignerRpc for VoteSignerRpcImpl {
    type Metadata = Meta;

    fn register(
        &self,
        meta: Self::Metadata,
        id: Pubkey,
        sig: Signature,
        signed_msg: Vec<u8>,
    ) -> Result<Pubkey> {
        info!("register rpc request received: {:?}", id);
        if sig.verify(id.as_ref(), signed_msg.as_ref()) {
            meta.request_processor.register(id)
        } else {
            Err(Error::invalid_request())
        }
    }

    fn sign(
        &self,
        meta: Self::Metadata,
        id: Pubkey,
        sig: Signature,
        signed_msg: Vec<u8>,
    ) -> Result<Signature> {
        info!("sign rpc request received: {:?}", id);
        if sig.verify(id.as_ref(), signed_msg.as_ref()) {
            meta.request_processor.sign(id, &signed_msg)
        } else {
            Err(Error::invalid_request())
        }
    }

    fn deregister(
        &self,
        meta: Self::Metadata,
        id: Pubkey,
        sig: Signature,
        signed_msg: Vec<u8>,
    ) -> Result<()> {
        info!("deregister rpc request received: {:?}", id);
        if sig.verify(id.as_ref(), signed_msg.as_ref()) {
            meta.request_processor.deregister(id)
        } else {
            Err(Error::invalid_request())
        }
    }
}

#[derive(Clone)]
pub struct VoteSignRequestProcessor {
    nodes: Arc<RwLock<HashMap<Pubkey, Keypair>>>,
}
impl VoteSignRequestProcessor {
    /// Process JSON-RPC request items sent via JSON-RPC.
    pub fn register(&self, pubkey: Pubkey) -> Result<Pubkey> {
        match self.nodes.read().unwrap().get(&pubkey) {
            Some(voting_keypair) => Ok(voting_keypair.pubkey()),
            None => {
                let voting_keypair = Keypair::new();
                let voting_pubkey = voting_keypair.pubkey();
                self.nodes.write().unwrap().insert(pubkey, voting_keypair);
                Ok(voting_pubkey)
            }
        }
    }
    pub fn sign(&self, pubkey: Pubkey, msg: &[u8]) -> Result<Signature> {
        match self.nodes.read().unwrap().get(&pubkey) {
            Some(voting_keypair) => {
                let sig = Signature::new(&voting_keypair.sign(&msg).as_ref());
                Ok(sig)
            }
            None => Err(Error::invalid_request()),
        }
    }
    pub fn deregister(&self, pubkey: Pubkey) -> Result<()> {
        self.nodes.write().unwrap().remove(&pubkey);
        Ok(())
    }
}

impl Default for VoteSignRequestProcessor {
    fn default() -> Self {
        VoteSignRequestProcessor {
            nodes: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

#[cfg(test)]
mod tests {}
