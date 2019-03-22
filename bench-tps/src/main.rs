mod bench;
mod cli;

use crate::bench::do_bench_tps;

fn main() {
    solana_logger::setup();
    solana_metrics::set_panic_hook("bench-tps");

    let matches = cli::build_args().get_matches();

    let cfg = cli::extract_args(&matches);

    do_bench_tps(cfg);
}
