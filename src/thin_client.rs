use solana_client::rpc_client::RpcClient;
use solana_runtime::bank_client::BankClient;
use solana_sdk::{
    client::SyncClient,
    message::Message,
    pubkey::Pubkey,
    signature::{Signature, Signer},
    signers::Signers,
    system_instruction,
    transaction::Transaction,
    transport::TransportError,
};

pub trait Client {
    fn send_and_confirm_message<S: Signers>(
        &self,
        message: Message,
        signers: &S,
    ) -> Result<Signature, TransportError>;

    fn get_balance1(&self, pubkey: &Pubkey) -> Result<u64, TransportError>;
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

    fn get_balance1(&self, pubkey: &Pubkey) -> Result<u64, TransportError> {
        let balance = self
            .get_balance(pubkey)
            .map_err(|e| TransportError::Custom(e.to_string()))?;
        Ok(balance)
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

    fn get_balance1(&self, pubkey: &Pubkey) -> Result<u64, TransportError> {
        self.get_balance(pubkey)
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

    fn get_balance1(&self, _pubkey: &Pubkey) -> Result<u64, TransportError> {
        Ok(0)
    }
}

pub struct ThinClient<C: Client>(pub C);

impl<C: Client> ThinClient<C> {
    pub fn transfer<S: Signer>(
        &self,
        lamports: u64,
        sender_keypair: &S,
        to_pubkey: &Pubkey,
    ) -> Result<Signature, TransportError> {
        let create_instruction =
            system_instruction::transfer(&sender_keypair.pubkey(), &to_pubkey, lamports);
        let message = Message::new(&[create_instruction]);
        self.send_message(message, &[sender_keypair])
    }
}

impl<C: Client> ThinClient<C> {
    pub fn send_message<S: Signers>(
        &self,
        message: Message,
        signers: &S,
    ) -> Result<Signature, TransportError> {
        self.0.send_and_confirm_message(message, signers)
    }

    pub fn get_balance(&self, pubkey: &Pubkey) -> Result<u64, TransportError> {
        self.0.get_balance1(pubkey)
    }
}
