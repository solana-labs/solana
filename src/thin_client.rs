use solana_client::rpc_client::RpcClient;
use solana_runtime::bank_client::BankClient;
use solana_sdk::{
    client::{AsyncClient, SyncClient},
    fee_calculator::FeeCalculator,
    hash::Hash,
    message::Message,
    pubkey::Pubkey,
    signature::{Keypair, Signature, Signer},
    signers::Signers,
    system_instruction,
    transaction::Transaction,
    transport::{Result, TransportError},
};

pub trait Client {
    fn send_and_confirm_transaction1(&self, transaction: Transaction) -> Result<Signature>;
    fn get_balance1(&self, pubkey: &Pubkey) -> Result<u64>;
    fn get_recent_blockhash_and_fees(&self) -> Result<(Hash, FeeCalculator)>;
}

impl Client for RpcClient {
    fn send_and_confirm_transaction1(&self, mut transaction: Transaction) -> Result<Signature> {
        let signers: Vec<&Keypair> = vec![]; // Don't allow resigning
        self.send_and_confirm_transaction_with_spinner(&mut transaction, &signers)
            .map_err(|e| TransportError::Custom(e.to_string()))
    }

    fn get_balance1(&self, pubkey: &Pubkey) -> Result<u64> {
        let balance = self
            .get_balance(pubkey)
            .map_err(|e| TransportError::Custom(e.to_string()))?;
        Ok(balance)
    }

    fn get_recent_blockhash_and_fees(&self) -> Result<(Hash, FeeCalculator)> {
        let blockhash = self
            .get_recent_blockhash()
            .map_err(|e| TransportError::Custom(e.to_string()))?;
        Ok(blockhash)
    }
}

impl Client for BankClient {
    fn send_and_confirm_transaction1(&self, transaction: Transaction) -> Result<Signature> {
        let signature = self.async_send_transaction(transaction)?;
        self.poll_for_signature(&signature)?;
        Ok(signature)
    }

    fn get_balance1(&self, pubkey: &Pubkey) -> Result<u64> {
        self.get_balance(pubkey)
    }

    fn get_recent_blockhash_and_fees(&self) -> Result<(Hash, FeeCalculator)> {
        self.get_recent_blockhash()
    }
}

pub struct ThinClient<C: Client>(pub C);

impl<C: Client> ThinClient<C> {
    pub fn send_transaction(&self, transaction: Transaction) -> Result<Signature> {
        self.0.send_and_confirm_transaction1(transaction)
    }

    pub fn send_message<S: Signers>(&self, message: Message, signers: &S) -> Result<Signature> {
        let (blockhash, _fee_caluclator) = self.0.get_recent_blockhash_and_fees()?;
        let transaction = Transaction::new(signers, message, blockhash);
        let signature = transaction.signatures[0];
        self.send_transaction(transaction)?;
        Ok(signature)
    }

    pub fn transfer<S: Signer>(
        &self,
        lamports: u64,
        sender_keypair: &S,
        to_pubkey: &Pubkey,
    ) -> Result<Signature> {
        let create_instruction =
            system_instruction::transfer(&sender_keypair.pubkey(), &to_pubkey, lamports);
        let message = Message::new(&[create_instruction]);
        self.send_message(message, &[sender_keypair])
    }

    pub fn get_recent_blockhash_and_fees(&self) -> Result<(Hash, FeeCalculator)> {
        self.0.get_recent_blockhash_and_fees()
    }

    pub fn get_balance(&self, pubkey: &Pubkey) -> Result<u64> {
        self.0.get_balance1(pubkey)
    }
}
