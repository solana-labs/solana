fn main() {
    solana_logger::setup_with_filter("solana=info");
    solana_core::perf_libs::init_cuda();
    solana_validator::main()
}
