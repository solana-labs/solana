use solana_runtime::bank::Bank;
use solana_sdk::genesis_block::create_genesis_block;
use solana_sdk::hash::hash;
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;
use std::thread::Builder;

#[test]
fn test_race_register_tick_freeze() {
    solana_logger::setup();

    let (mut genesis_block, _) = create_genesis_block(50);
    genesis_block.ticks_per_slot = 1;
    let p = Pubkey::new_rand();
    let hash = hash(p.as_ref());

    for _ in 0..1000 {
        let bank0 = Arc::new(Bank::new(&genesis_block));
        let bank0_ = bank0.clone();
        let freeze_thread = Builder::new()
            .name("freeze".to_string())
            .spawn(move || loop {
                if bank0_.tick_height() == bank0_.max_tick_height() {
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
