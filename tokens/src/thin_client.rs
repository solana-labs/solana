use solana_client::{rpc_client::RpcClient, rpc_config::RpcSendTransactionConfig};
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
    fn send_transaction1(&self, transaction: Transaction) -> Result<Signature>;
    fn get_signature_statuses1(
        &self,
        signatures: &[Signature],
    ) -> Result<Vec<Option<TransactionStatus>>>;
    fn get_balance1(&self, pubkey: &Pubkey) -> Result<u64>;
    fn get_recent_blockhash1(&self) -> Result<(Hash, FeeCalculator)>;
    fn get_account1(&self, pubkey: &Pubkey) -> Result<Option<Account>>;
}

impl Client for RpcClient {
    fn send_transaction1(&self, transaction: Transaction) -> Result<Signature> {
        self.send_transaction_with_config(
            &transaction,
            RpcSendTransactionConfig {
                skip_preflight: true,
            },
        )
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
    fn send_transaction1(&self, transaction: Transaction) -> Result<Signature> {
        self.async_send_transaction(transaction)
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

pub struct ThinClient<C: Client> {
    client: C,
    dry_run: bool,
}

impl<C: Client> ThinClient<C> {
    pub fn new(client: C, dry_run: bool) -> Self {
        Self { client, dry_run }
    }

    pub fn send_transaction(&self, transaction: Transaction) -> Result<Signature> {
        if self.dry_run {
            return Ok(Signature::default());
        }
        self.client.send_transaction1(transaction)
    }

    pub fn poll_for_confirmation(&self, signature: &Signature) -> Result<()> {
        while self.get_signature_statuses(&[*signature])?[0].is_none() {
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        Ok(())
    }

    pub fn get_signature_statuses(
        &self,
        signatures: &[Signature],
    ) -> Result<Vec<Option<TransactionStatus>>> {
        self.client.get_signature_statuses1(signatures)
    }

    pub fn send_message<S: Signers>(&self, message: Message, signers: &S) -> Result<Transaction> {
        if self.dry_run {
            return Ok(Transaction::new_unsigned(message));
        }
        let (blockhash, _fee_caluclator) = self.get_recent_blockhash()?;
        let transaction = Transaction::new(signers, message, blockhash);
        self.send_transaction(transaction.clone())?;
        Ok(transaction)
    }

    pub fn transfer<S: Signer>(
        &self,
        lamports: u64,
        sender_keypair: &S,
        to_pubkey: &Pubkey,
    ) -> Result<Transaction> {
        let create_instruction =
            system_instruction::transfer(&sender_keypair.pubkey(), &to_pubkey, lamports);
        let message = Message::new(&[create_instruction]);
        self.send_message(message, &[sender_keypair])
    }

    pub fn get_recent_blockhash(&self) -> Result<(Hash, FeeCalculator)> {
        self.client.get_recent_blockhash1()
    }

    pub fn get_balance(&self, pubkey: &Pubkey) -> Result<u64> {
        self.client.get_balance1(pubkey)
    }

    pub fn get_account(&self, pubkey: &Pubkey) -> Result<Option<Account>> {
        self.client.get_account1(pubkey)
    }

    pub fn get_recent_blockhashes(&self) -> Result<Vec<Hash>> {
        let opt_blockhashes_account = self.get_account(&recent_blockhashes::id())?;
        let blockhashes_account = opt_blockhashes_account.unwrap();
        let recent_blockhashes = RecentBlockhashes::from_account(&blockhashes_account).unwrap();
        let hashes = recent_blockhashes.iter().map(|x| x.blockhash).collect();
        Ok(hashes)
    }
}
