use {
    crate::bench_tps_client::{BenchTpsClient, Result},
    log::*,
    solana_sdk::{
        clock::DEFAULT_MS_PER_SLOT, commitment_config::CommitmentConfig, slot_history::Slot,
    },
    std::{sync::Arc, thread::sleep, time::Duration},
};

const NUM_RETRY: u64 = 5;
const RETRY_EVERY_MS: u64 = 4 * DEFAULT_MS_PER_SLOT;

fn call_rpc_with_retry<Func, Data>(f: Func, retry_warning: &str) -> Result<Data>
where
    Func: Fn() -> Result<Data>,
{
    let mut iretry = 0;
    loop {
        match f() {
            Ok(slot) => {
                return Ok(slot);
            }
            Err(error) => {
                if iretry == NUM_RETRY {
                    return Err(error);
                }
                warn!("{retry_warning}: {error}, retry.");
                sleep(Duration::from_millis(RETRY_EVERY_MS));
            }
        }
        iretry += 1;
    }
}

pub(crate) fn get_slot_with_retry<Client>(
    client: &Arc<Client>,
    commitment: CommitmentConfig,
) -> Result<Slot>
where
    Client: 'static + BenchTpsClient + Send + Sync + ?Sized,
{
    call_rpc_with_retry(
        || client.get_slot_with_commitment(commitment),
        "Failed to get slot",
    )
}

pub(crate) fn get_blocks_with_retry<Client>(
    client: &Arc<Client>,
    start_slot: Slot,
    end_slot: Option<Slot>,
    commitment: CommitmentConfig,
) -> Result<Vec<Slot>>
where
    Client: 'static + BenchTpsClient + Send + Sync + ?Sized,
{
    call_rpc_with_retry(
        || client.get_blocks_with_commitment(start_slot, end_slot, commitment),
        "Failed to download blocks",
    )
}
