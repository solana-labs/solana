fn main() -> Result<(), String> {
    println!(
        "⚠️  solana-install is deprecated and will be discontinued when v1.18 is no longer \
        supported. Please switch to Agave: \
        https://github.com/anza-xyz/agave/wiki/Agave-Transition\n"
    );
    solana_install::main()
}
