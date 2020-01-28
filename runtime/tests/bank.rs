use solana_runtime::bank::Bank;
use solana_sdk::{genesis_config::create_genesis_config, hash::hash, pubkey::Pubkey};
use std::{sync::Arc, thread::Builder};

#[test]
fn test_race_register_tick_freeze() {
    solana_logger::setup();

    let (mut genesis_config, _) = create_genesis_config(50);
    genesis_config.ticks_per_slot = 1;
    let p = Pubkey::new_rand();
    let hash = hash(p.as_ref());

    for _ in 0..1000 {
        let bank0 = Arc::new(Bank::new(&genesis_config));
        let bank0_ = bank0.clone();
        let freeze_thread = Builder::new()
            .name("freeze".to_string())
            .spawn(move || loop {
                if bank0_.is_complete() {
                    assert_eq!(bank0_.last_blockhash(), hash);
                    break;
                }
            })
            .unwrap();

        let bank0_ = bank0.clone();
        let register_tick_thread = Builder::new()
            .name("register_tick".to_string())
            .spawn(move || {
                bank0_.register_tick(&hash);
            })
            .unwrap();

        register_tick_thread.join().unwrap();
        freeze_thread.join().unwrap();
    }
}
