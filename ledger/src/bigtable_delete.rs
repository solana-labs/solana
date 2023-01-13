use {log::*, solana_measure::measure::Measure, solana_sdk::clock::Slot, std::result::Result};

// Attempt to delete this many blocks in parallel
const NUM_BLOCKS_TO_DELETE_IN_PARALLEL: usize = 32;

pub async fn delete_confirmed_blocks(
    bigtable: solana_storage_bigtable::LedgerStorage,
    blocks_to_delete: Vec<Slot>,
    dry_run: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut measure = Measure::start("entire delete");

    if blocks_to_delete.is_empty() {
        info!("No blocks to be deleted");
        return Ok(());
    }
    info!("{} blocks to be deleted", blocks_to_delete.len());

    let mut failures = 0;
    for blocks in blocks_to_delete.chunks(NUM_BLOCKS_TO_DELETE_IN_PARALLEL) {
        let mut measure_delete = Measure::start("Delete");
        info!("Preparing the next {} blocks for deletion", blocks.len());

        let deletion_futures = blocks
            .iter()
            .map(|block| bigtable.delete_confirmed_block(*block, dry_run));

        for (block, result) in blocks
            .iter()
            .zip(futures::future::join_all(deletion_futures).await)
        {
            if result.is_err() {
                error!(
                    "delete_confirmed_block({}) failed: {:?}",
                    block,
                    result.err()
                );
                failures += 1;
            }
        }

        measure_delete.stop();
        info!("{} for {} blocks", measure_delete, blocks.len());
    }

    measure.stop();
    info!("{}", measure);
    if failures > 0 {
        Err(format!("Incomplete deletion, {failures} operations failed").into())
    } else {
        Ok(())
    }
}
