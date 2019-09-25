fn main() {
    solana_core::perf_libs::init_cuda();
    solana_validator::main()
}
