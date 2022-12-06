use {
    crate::{
        bench::{fund_keypairs, generate_and_fund_keypairs},
        bench_tps_client::BenchTpsClient,
    },
    log::*,
    solana_genesis::Base64Account,
    solana_sdk::signature::{Keypair, Signer},
    std::{collections::HashMap, fs::File, path::Path, process::exit, sync::Arc},
};

pub fn get_keypairs<T>(
    client: Arc<T>,
    id: &Keypair,
    keypair_count: usize,
    num_lamports_per_account: u64,
    client_ids_and_stake_file: &str,
    read_from_client_file: bool,
) -> Vec<Keypair>
where
    T: 'static + BenchTpsClient + Send + Sync + ?Sized,
{
    if read_from_client_file {
        let path = Path::new(client_ids_and_stake_file);
        let file = File::open(path).unwrap();

        info!("Reading {}", client_ids_and_stake_file);
        let accounts: HashMap<String, Base64Account> = serde_yaml::from_reader(file).unwrap();
        let mut keypairs = vec![];
        let mut last_balance = 0;

        accounts
            .into_iter()
            .for_each(|(keypair, primordial_account)| {
                let bytes: Vec<u8> = serde_json::from_str(keypair.as_str()).unwrap();
                keypairs.push(Keypair::from_bytes(&bytes).unwrap());
                last_balance = primordial_account.balance;
            });

        if keypairs.len() < keypair_count {
            eprintln!(
                "Expected {} accounts in {}, only received {} (--tx_count mismatch?)",
                keypair_count,
                client_ids_and_stake_file,
                keypairs.len(),
            );
            exit(1);
        }
        // Sort keypairs so that do_bench_tps() uses the same subset of accounts for each run.
        // This prevents the amount of storage needed for bench-tps accounts from creeping up
        // across multiple runs.
        keypairs.sort_by_key(|x| x.pubkey().to_string());
        fund_keypairs(
            client,
            id,
            &keypairs,
            keypairs.len().saturating_sub(keypair_count) as u64,
            last_balance,
        )
        .unwrap_or_else(|e| {
            eprintln!("Error could not fund keys: {e:?}");
            exit(1);
        });
        keypairs
    } else {
        generate_and_fund_keypairs(client, id, keypair_count, num_lamports_per_account)
            .unwrap_or_else(|e| {
                eprintln!("Error could not fund keys: {e:?}");
                exit(1);
            })
    }
}
