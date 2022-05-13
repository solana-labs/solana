use {
    crate::blockstore::Blockstore,
    crossbeam_channel::bounded,
    log::*,
    solana_measure::measure::Measure,
    solana_sdk::clock::Slot,
    std::{
        cmp::min,
        collections::HashSet,
        result::Result,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        time::Duration,
    },
};

#[derive(Clone)]
pub struct ConfirmedBlockUploadConfig {
    pub force_reupload: bool,
    pub max_num_slots_to_check: usize,
    pub num_blocks_to_upload_in_parallel: usize,
    pub block_read_ahead_depth: usize, // should always be >= `num_blocks_to_upload_in_parallel`
}

impl Default for ConfirmedBlockUploadConfig {
    fn default() -> Self {
        const NUM_BLOCKS_TO_UPLOAD_IN_PARALLEL: usize = 32;
        ConfirmedBlockUploadConfig {
            force_reupload: false,
            max_num_slots_to_check: NUM_BLOCKS_TO_UPLOAD_IN_PARALLEL * 4,
            num_blocks_to_upload_in_parallel: NUM_BLOCKS_TO_UPLOAD_IN_PARALLEL,
            block_read_ahead_depth: NUM_BLOCKS_TO_UPLOAD_IN_PARALLEL * 2,
        }
    }
}

pub async fn upload_confirmed_blocks(
    blockstore: Arc<Blockstore>,
    bigtable: solana_storage_bigtable::LedgerStorage,
    starting_slot: Slot,
    ending_slot: Option<Slot>,
    config: ConfirmedBlockUploadConfig,
    exit: Arc<AtomicBool>,
) -> Result<Slot, Box<dyn std::error::Error>> {
    let mut measure = Measure::start("entire upload");

    info!("Loading ledger slots starting at {}...", starting_slot);
    let blockstore_slots: Vec<_> = blockstore
        .rooted_slot_iterator(starting_slot)
        .map_err(|err| {
            format!(
                "Failed to load entries starting from slot {}: {:?}",
                starting_slot, err
            )
        })?
        .map_while(|slot| {
            if let Some(ending_slot) = &ending_slot {
                if slot > *ending_slot {
                    return None;
                }
            }
            Some(slot)
        })
        .collect();

    if blockstore_slots.is_empty() {
        return Err(format!(
            "Ledger has no slots from {} to {:?}",
            starting_slot, ending_slot
        )
        .into());
    }

    let first_blockstore_slot = blockstore_slots.first().unwrap();
    let last_blockstore_slot = blockstore_slots.last().unwrap();
    info!(
        "Found {} slots in the range ({}, {})",
        blockstore_slots.len(),
        first_blockstore_slot,
        last_blockstore_slot,
    );

    // Gather the blocks that are already present in bigtable, by slot
    let bigtable_slots = if !config.force_reupload {
        let mut bigtable_slots = vec![];
        info!(
            "Loading list of bigtable blocks between slots {} and {}...",
            first_blockstore_slot, last_blockstore_slot
        );

        let mut start_slot = *first_blockstore_slot;
        while start_slot <= *last_blockstore_slot {
            let mut next_bigtable_slots = loop {
                let num_bigtable_blocks = min(1000, config.max_num_slots_to_check * 2);
                match bigtable
                    .get_confirmed_blocks(start_slot, num_bigtable_blocks)
                    .await
                {
                    Ok(slots) => break slots,
                    Err(err) => {
                        error!("get_confirmed_blocks for {} failed: {:?}", start_slot, err);
                        // Consider exponential backoff...
                        tokio::time::sleep(Duration::from_secs(2)).await;
                    }
                }
            };
            if next_bigtable_slots.is_empty() {
                break;
            }
            bigtable_slots.append(&mut next_bigtable_slots);
            start_slot = bigtable_slots.last().unwrap() + 1;
        }
        bigtable_slots
            .into_iter()
            .filter(|slot| slot <= last_blockstore_slot)
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };

    // The blocks that still need to be uploaded is the difference between what's already in the
    // bigtable and what's in blockstore...
    let blocks_to_upload = {
        let blockstore_slots = blockstore_slots.iter().cloned().collect::<HashSet<_>>();
        let bigtable_slots = bigtable_slots.into_iter().collect::<HashSet<_>>();

        let mut blocks_to_upload = blockstore_slots
            .difference(&bigtable_slots)
            .cloned()
            .collect::<Vec<_>>();
        blocks_to_upload.sort_unstable();
        blocks_to_upload.truncate(config.max_num_slots_to_check);
        blocks_to_upload
    };

    if blocks_to_upload.is_empty() {
        info!("No blocks need to be uploaded to bigtable");
        return Ok(*last_blockstore_slot);
    }
    let last_slot = *blocks_to_upload.last().unwrap();
    info!(
        "{} blocks to be uploaded to the bucket in the range ({}, {})",
        blocks_to_upload.len(),
        blocks_to_upload.first().unwrap(),
        last_slot
    );

    // Load the blocks out of blockstore in a separate thread to allow for concurrent block uploading
    let (_loader_thread, receiver) = {
        let exit = exit.clone();

        let (sender, receiver) = bounded(config.block_read_ahead_depth);
        (
            std::thread::spawn(move || {
                let mut measure = Measure::start("block loader thread");
                for (i, slot) in blocks_to_upload.iter().enumerate() {
                    if exit.load(Ordering::Relaxed) {
                        break;
                    }

                    let _ = match blockstore.get_rooted_block(*slot, true) {
                        Ok(confirmed_block) => sender.send((*slot, Some(confirmed_block))),
                        Err(err) => {
                            warn!(
                                "Failed to get load confirmed block from slot {}: {:?}",
                                slot, err
                            );
                            sender.send((*slot, None))
                        }
                    };

                    if i > 0 && i % config.num_blocks_to_upload_in_parallel == 0 {
                        info!(
                            "{}% of blocks processed ({}/{})",
                            i * 100 / blocks_to_upload.len(),
                            i,
                            blocks_to_upload.len()
                        );
                    }
                }
                measure.stop();
                info!("{} to load {} blocks", measure, blocks_to_upload.len());
            }),
            receiver,
        )
    };

    let mut failures = 0;
    use futures::stream::StreamExt;

    let mut stream =
        tokio_stream::iter(receiver.into_iter()).chunks(config.num_blocks_to_upload_in_parallel);

    while let Some(blocks) = stream.next().await {
        if exit.load(Ordering::Relaxed) {
            break;
        }

        let mut measure_upload = Measure::start("Upload");
        let mut num_blocks = blocks.len();
        info!("Preparing the next {} blocks for upload", num_blocks);

        let uploads = blocks.into_iter().filter_map(|(slot, block)| match block {
            None => {
                num_blocks -= 1;
                None
            }
            Some(confirmed_block) => Some(bigtable.upload_confirmed_block(slot, confirmed_block)),
        });

        for result in futures::future::join_all(uploads).await {
            if result.is_err() {
                error!("upload_confirmed_block() failed: {:?}", result.err());
                failures += 1;
            }
        }

        measure_upload.stop();
        info!("{} for {} blocks", measure_upload, num_blocks);
    }

    measure.stop();
    info!("{}", measure);
    if failures > 0 {
        Err(format!("Incomplete upload, {} operations failed", failures).into())
    } else {
        Ok(last_slot)
    }
}
