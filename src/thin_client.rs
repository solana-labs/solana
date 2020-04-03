use solana_client::rpc_client::RpcClient;
use solana_runtime::bank_client::BankClient;
use solana_sdk::client::SyncClient;
use solana_sdk::{
    message::Message, signature::Signature, signers::Signers, transaction::Transaction,
    transport::TransportError,
};

pub trait Client {
    fn send_and_confirm_message<S: Signers>(
        &self,
        message: Message,
        signers: &S,
    ) -> Result<Signature, TransportError>;
}

impl Client for RpcClient {
    fn send_and_confirm_message<S: Signers>(
        &self,
        message: Message,
        signers: &S,
    ) -> Result<Signature, TransportError> {
        let mut transaction = Transaction::new_unsigned(message);
        self.resign_transaction(&mut transaction, signers)
            .map_err(|e| TransportError::Custom(e.to_string()))?;
        let signature = self
            .send_and_confirm_transaction_with_spinner(&mut transaction, signers)
            .map_err(|e| TransportError::Custom(e.to_string()))?;
        Ok(signature)
    }
}

impl Client for BankClient {
    fn send_and_confirm_message<S: Signers>(
        &self,
        message: Message,
        signers: &S,
    ) -> Result<Signature, TransportError> {
        self.send_message(signers, message)
    }
}

impl Client for () {
    fn send_and_confirm_message<S: Signers>(
        &self,
        _message: Message,
        _signers: &S,
    ) -> Result<Signature, TransportError> {
        Ok(Signature::default())
    }
}

pub struct ThinClient<C: Client>(pub C);

impl<C: Client> ThinClient<C> {
    pub fn send_message<S: Signers>(
        &self,
        message: Message,
        signers: &S,
    ) -> Result<Signature, TransportError> {
        self.0.send_and_confirm_message(message, signers)
    }
}
