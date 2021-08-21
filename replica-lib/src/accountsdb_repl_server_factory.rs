use {
    crate::{
        accountsdb_repl_server::{AccountsDbReplService, AccountsDbReplServiceConfig},
        replica_accounts_server::ReplicaAccountsServerImpl,
        replica_confirmed_slots_server::ReplicaSlotConfirmationServerImpl,
    },
    crossbeam_channel::Receiver,
    solana_runtime::bank_forks::BankForks,
    solana_sdk::clock::Slot,
    std::sync::{Arc, RwLock},
};

pub struct AccountsDbReplServerFactory {}

impl AccountsDbReplServerFactory {
    pub fn build_accountsdb_repl_server(
        config: AccountsDbReplServiceConfig,
        confirmed_bank_receiver: Receiver<Slot>,
        bank_forks: Arc<RwLock<BankForks>>,
    ) -> AccountsDbReplService {
        AccountsDbReplService::new(
            config,
            Arc::new(RwLock::new(ReplicaSlotConfirmationServerImpl::new(
                confirmed_bank_receiver,
            ))),
            Arc::new(RwLock::new(ReplicaAccountsServerImpl::new(bank_forks))),
        )
    }
}
