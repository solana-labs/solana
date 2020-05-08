use solana_client::rpc_client::RpcClient;
use solana_runtime::bank_client::BankClient;
use solana_sdk::{
    account::Account,
    client::{AsyncClient, SyncClient},
    fee_calculator::FeeCalculator,
    hash::Hash,
    message::Message,
    pubkey::Pubkey,
    signature::{Signature, Signer},
    signers::Signers,
    system_instruction,
    sysvar::{
        recent_blockhashes::{self, RecentBlockhashes},
        Sysvar,
    },
    transaction::Transaction,
    transport::{Result, TransportError},
};
use solana_transaction_status::TransactionStatus;

pub trait Client {
    fn async_send_transaction1(&self, transaction: Transaction) -> Result<Signature>;

    // TODO: Work to delete this
    fn send_and_confirm_transaction1(&self, transaction: Transaction) -> Result<Signature>;

    fn get_signature_statuses1(
        &self,
        signatures: &[Signature],
    ) -> Result<Vec<Option<TransactionStatus>>>;
    fn get_balance1(&self, pubkey: &Pubkey) -> Result<u64>;
    fn get_recent_blockhash1(&self) -> Result<(Hash, FeeCalculator)>;
    fn get_account1(&self, pubkey: &Pubkey) -> Result<Option<Account>>;
}

impl Client for RpcClient {
    fn async_send_transaction1(&self, transaction: Transaction) -> Result<Signature> {
        self.send_transaction(&transaction)
            .map_err(|e| TransportError::Custom(e.to_string()))
    }

    fn send_and_confirm_transaction1(&self, mut transaction: Transaction) -> Result<Signature> {
        let signers: Vec<&dyn Signer> = vec![]; // Don't allow resigning
        self.send_and_confirm_transaction_with_spinner(&mut transaction, &signers)
            .map_err(|e| TransportError::Custom(e.to_string()))
    }

    fn get_signature_statuses1(
        &self,
        signatures: &[Signature],
    ) -> Result<Vec<Option<TransactionStatus>>> {
        self.get_signature_statuses(signatures)
            .map(|response| response.value)
            .map_err(|e| TransportError::Custom(e.to_string()))
    }

    fn get_balance1(&self, pubkey: &Pubkey) -> Result<u64> {
        self.get_balance(pubkey)
            .map_err(|e| TransportError::Custom(e.to_string()))
    }

    fn get_recent_blockhash1(&self) -> Result<(Hash, FeeCalculator)> {
        self.get_recent_blockhash()
            .map_err(|e| TransportError::Custom(e.to_string()))
    }

    fn get_account1(&self, pubkey: &Pubkey) -> Result<Option<Account>> {
        self.get_account(pubkey)
            .map(Some)
            .map_err(|e| TransportError::Custom(e.to_string()))
    }
}

impl Client for BankClient {
    fn async_send_transaction1(&self, transaction: Transaction) -> Result<Signature> {
        self.async_send_transaction(transaction)
    }

    fn send_and_confirm_transaction1(&self, transaction: Transaction) -> Result<Signature> {
        let signature = self.async_send_transaction1(transaction)?;
        self.poll_for_signature(&signature)?;
        Ok(signature)
    }

    fn get_signature_statuses1(
        &self,
        signatures: &[Signature],
    ) -> Result<Vec<Option<TransactionStatus>>> {
        signatures
            .iter()
            .map(|signature| {
                self.get_signature_status(signature).map(|opt| {
                    opt.map(|status| TransactionStatus {
                        slot: 0,
                        confirmations: None,
                        status,
                        err: None,
                    })
                })
            })
            .collect()
    }

    fn get_balance1(&self, pubkey: &Pubkey) -> Result<u64> {
        self.get_balance(pubkey)
    }

    fn get_recent_blockhash1(&self) -> Result<(Hash, FeeCalculator)> {
        self.get_recent_blockhash()
    }

    fn get_account1(&self, pubkey: &Pubkey) -> Result<Option<Account>> {
        self.get_account(pubkey)
    }
}

pub struct ThinClient<C: Client>(C);

impl<C: Client> ThinClient<C> {
    pub fn new(client: C) -> Self {
        Self(client)
    }

    pub fn async_send_transaction(&self, transaction: Transaction) -> Result<Signature> {
        self.0.async_send_transaction1(transaction)
    }

    pub fn get_signature_statuses(
        &self,
        signatures: &[Signature],
    ) -> Result<Vec<Option<TransactionStatus>>> {
        self.0.get_signature_statuses1(signatures)
    }

    pub fn send_transaction(&self, transaction: Transaction) -> Result<Signature> {
        // TODO: implement this in terms of ThinClient methods and then remove
        // send_and_confirm_transaction1 from from the Client trait.
        self.0.send_and_confirm_transaction1(transaction)
    }

    pub fn send_message<S: Signers>(&self, message: Message, signers: &S) -> Result<Signature> {
        let (blockhash, _fee_caluclator) = self.get_recent_blockhash()?;
        let transaction = Transaction::new(signers, message, blockhash);
        self.send_transaction(transaction)
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

    pub fn get_recent_blockhash(&self) -> Result<(Hash, FeeCalculator)> {
        self.0.get_recent_blockhash1()
    }

    pub fn get_balance(&self, pubkey: &Pubkey) -> Result<u64> {
        self.0.get_balance1(pubkey)
    }

    pub fn get_account(&self, pubkey: &Pubkey) -> Result<Option<Account>> {
        self.0.get_account1(pubkey)
    }

    pub fn get_recent_blockhashes(&self) -> Result<Vec<Hash>> {
        let opt_blockhashes_account = self.get_account(&recent_blockhashes::id())?;
        let blockhashes_account = opt_blockhashes_account.unwrap();
        let recent_blockhashes = RecentBlockhashes::from_account(&blockhashes_account).unwrap();
        let hashes = recent_blockhashes.iter().map(|x| x.blockhash).collect();
        Ok(hashes)
    }
}
