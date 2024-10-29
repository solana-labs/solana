pub use solana_nonce::{state::State, NONCED_TX_MARKER_IX_INDEX};
pub mod state {
    pub use solana_nonce::{
        state::{Data, DurableNonce, State},
        versions::{AuthorizeNonceError, Versions},
    };
}
