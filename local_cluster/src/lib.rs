#[cfg(test)]
mod cluster;
#[cfg(test)]
mod cluster_tests;
#[cfg(test)]
mod local_cluster;
#[cfg(test)]
mod tests;

#[macro_use]
#[cfg(test)]
extern crate log;

#[cfg(test)]
extern crate solana_bench_exchange;

#[cfg(test)]
extern crate solana_bench_tps;

#[macro_use]
#[cfg(test)]
extern crate solana_core;

#[cfg(test)]
extern crate solana_drone;

#[macro_use]
#[cfg(test)]
extern crate solana_exchange_program;

#[macro_use]
#[cfg(test)]
extern crate solana_move_loader_program;

#[macro_use]
#[cfg(test)]
extern crate solana_storage_program;

#[cfg(test)]
extern crate tempfile;
