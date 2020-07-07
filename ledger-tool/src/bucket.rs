/// The `bucket` subcommand
use log::*;
use solana_bucket_ledger::BucketLedger;
use solana_ledger::blockstore::Blockstore;
use solana_measure::measure::Measure;
use solana_sdk::clock::Slot;
use std::{collections::HashSet, result::Result, time::Duration};
use tokio::time::delay_for;

pub async fn do_upload_to_bucket(
    blockstore: Blockstore,
    bucket_ledger: BucketLedger,
    starting_slot: Slot,
    ending_slot: Option<Slot>,
) -> Result<(), String> {
    // Load the slots to be uploaded from blockstore
    info!("Loading slots from ledger...");
    let blockstore_slots: Vec<_> = blockstore
        .slot_meta_iterator(starting_slot)
        .map_err(|err| {
            format!(
                "Failed to load entries starting from slot {}: {:?}",
                starting_slot, err
            )
        })?
        .filter_map(|(slot, _slot_meta)| {
            if let Some(ending_slot) = &ending_slot {
                if slot > *ending_slot {
                    return None;
                }
            }
            Some(slot)
        })
        .collect();

    if blockstore_slots.is_empty() {
        info!("Ledger has no slots to upload");
        return Ok(());
    }
    info!(
        "Ledger has {} slots in the range ({}, {})",
        blockstore_slots.len(),
        blockstore_slots.first().unwrap(),
        blockstore_slots.last().unwrap()
    );

    let mut blockstore_slots_with_no_confirmed_block = HashSet::new();
    loop {
        // List the bucket and gather the slots that are already present
        info!("Loading slots from bucket...");
        let bucket_slots = {
            let mut bucket_slots = vec![];
            let mut start_slot = *blockstore_slots.first().unwrap();
            let last_blockstore_slot = *blockstore_slots.last().unwrap();

            while start_slot <= last_blockstore_slot {
                let mut next_bucket_slots = loop {
                    match bucket_ledger.get_confirmed_blocks(start_slot, None).await {
                        Ok(slots) => break slots,
                        Err(err) => {
                            error!("get_confirmed_blocks for {} failed: {:?}", start_slot, err);
                            // Consider exponential backoff?
                            delay_for(Duration::from_secs(2)).await;
                        }
                    }
                };
                if next_bucket_slots.is_empty() {
                    break;
                }
                bucket_slots.append(&mut next_bucket_slots);
                start_slot = bucket_slots.last().unwrap() + 1;
            }
            bucket_slots
                .into_iter()
                .filter(|slot| *slot <= last_blockstore_slot)
                .collect::<Vec<_>>()
        };

        // The slots that still need to be uploaded is the difference between what's already in the
        // bucket and what's in blockstore...
        let slots_to_upload = {
            let blockstore_slots = blockstore_slots.iter().cloned().collect::<HashSet<_>>();
            let bucket_slots = bucket_slots.into_iter().collect::<HashSet<_>>();

            let mut slots_to_upload = blockstore_slots
                .difference(&blockstore_slots_with_no_confirmed_block)
                .cloned()
                .collect::<HashSet<_>>()
                .difference(&bucket_slots)
                .cloned()
                .collect::<Vec<_>>();
            slots_to_upload.sort();
            slots_to_upload
        };

        if slots_to_upload.is_empty() {
            info!("No (more) slots need to be uploaded");
            return Ok(());
        }
        info!(
            "{} slots to be uploaded to the bucket in the range ({}, {})",
            slots_to_upload.len(),
            slots_to_upload.first().unwrap(),
            slots_to_upload.last().unwrap()
        );

        // Upload the slots in chunks
        for slot_chunk in slots_to_upload.chunks(1000) {
            let mut measure_load = Measure::start("blockstore load");

            // Read the blocks from blockstore
            let blocks: Vec<_> = slot_chunk
                .iter()
                .filter_map(|slot| {
                    match blockstore.get_confirmed_block(
                        *slot,
                        Some(solana_transaction_status::UiTransactionEncoding::Binary),
                    ) {
                        Ok(confirmed_block) => Some((*slot, confirmed_block)),
                        Err(err) => {
                            warn!(
                                "Failed to get load confirmed block from slot {}: {:?}",
                                slot, err
                            );
                            blockstore_slots_with_no_confirmed_block.insert(*slot);
                            None
                        }
                    }
                })
                .collect();

            measure_load.stop();
            info!(
                "{} for {} blocks in the range ({}, {})",
                measure_load,
                blocks.len(),
                slot_chunk.first().unwrap(),
                slot_chunk.last().unwrap()
            );

            // Upload them all to the bucket
            let mut measure_upload = Measure::start("upload");
            let num_blocks = blocks.len();
            let mut num_errors = 0;

            let uploads = blocks
                .into_iter()
                .map(|(slot, block)| bucket_ledger.upload_confirmed_block(slot, block));

            for result in futures::future::join_all(uploads).await {
                if result.is_err() {
                    // TODO: Emit a datapoint
                    error!("upload_confirmed_block() failed: {:?}", result.err());
                    num_errors += 1;
                }
            }

            measure_upload.stop();
            info!(
                "{} for {} blocks with {} errors",
                measure_upload, num_blocks, num_errors
            );

            if num_errors > 0 {
                // Consider exponential backoff?
                warn!("delaying due to {} upload errors...", num_errors);
                delay_for(Duration::from_secs(2)).await;
            }
        }
    }
}

pub async fn do_test(bucket_ledger: BucketLedger) -> solana_bucket_ledger::Result<()> {
    let mut measure = Measure::start("");
    println!(
        "get_first_available_block: {:?}",
        bucket_ledger.get_first_available_block().await?
    );
    measure.stop();
    println!("^{}\n", measure);

    let mut measure = Measure::start("");
    let slot = 2_100_000;
    println!(
        "get_confirmed_blocks({}): {:?}",
        slot,
        bucket_ledger.get_confirmed_blocks(slot, None).await?
    );
    measure.stop();
    println!("^{}\n", measure);

    let mut measure = Measure::start("");
    let confirmed_block = bucket_ledger
        .get_confirmed_block(slot, solana_transaction_status::TransactionEncoding::Json)
        .await?;
    println!("get_confirmed_block({}): {:?}", slot, confirmed_block);
    measure.stop();
    println!("^{}\n", measure);

    let signature =
        "54BHfkKaU465FN15CTQCR9rbbCK3JBUruS85bsQ9ub86qJmx35SfM5XT5wQ5bpuTjpjS85uztWfbgAGJz1AqQu5U"
            .parse()
            .unwrap();

    let mut measure = Measure::start("");
    let confirmed_block = bucket_ledger.get_transaction_status(&signature).await?;
    println!(
        "get_transaction_status({}): {:?}",
        signature, confirmed_block
    );
    measure.stop();
    println!("^{}\n", measure);

    let signature2 =
        "5EqMoDdmWBRCCgV6opUnGM3X5AtEwdGFhaBr81tWA7AStPpG6cSvgvNxyme9VUmGY86xXTcQM8nZGDMRqs7v178D"
            .parse()
            .unwrap();

    let mut measure = Measure::start("");
    let confirmed_block = bucket_ledger
        .get_confirmed_transaction(
            &signature2,
            solana_transaction_status::TransactionEncoding::Json,
        )
        .await?;
    println!(
        "get_confirmed_transaction({}): {:?}",
        signature2, confirmed_block
    );
    measure.stop();
    println!("^{}\n", measure);

    let address = "Vote111111111111111111111111111111111111111"
        .parse()
        .unwrap();
    let mut measure = Measure::start("");
    println!(
        ".get_confirmed_signatures_for_address({}, {}): {:?}",
        address,
        signature,
        bucket_ledger
            .get_confirmed_signatures_for_address(&address, Some(&signature), Some(10))
            .await?
    );
    measure.stop();
    println!("^{}\n", measure);

    Ok(())
}
