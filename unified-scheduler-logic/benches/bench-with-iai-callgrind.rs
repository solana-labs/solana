use {
    assert_matches::assert_matches,
    iai_callgrind::{
        client_requests::callgrind::toggle_collect, library_benchmark, library_benchmark_group,
        main,
    },
    solana_sdk::{
        instruction::{AccountMeta, Instruction},
        message::Message,
        pubkey::Pubkey,
        signature::Signer,
        signer::keypair::Keypair,
        system_transaction,
        transaction::{Result, SanitizedTransaction, Transaction},
    },
    solana_unified_scheduler_logic::{Page, SchedulingStateMachine, Task},
    std::hint::black_box,
};

#[library_benchmark]
#[bench::min(0)]
#[bench::one(1)]
#[bench::two(2)]
#[bench::three(3)]
#[bench::normal(32)]
#[bench::large(64)]
#[bench::max(128)]
fn bench_schedule_task(account_count: usize) {
    toggle_collect();
    let mut accounts = vec![];
    for _ in 0..account_count {
        accounts.push(AccountMeta::new(Keypair::new().pubkey(), true));
    }

    let payer = Keypair::new();
    let memo_ix = Instruction {
        program_id: Pubkey::default(),
        accounts,
        data: vec![0x00],
    };
    let mut ixs = vec![];
    for _ in 0..1 {
        ixs.push(memo_ix.clone());
    }
    let msg = Message::new(&ixs, Some(&payer.pubkey()));
    let txn = Transaction::new_unsigned(msg);
    //panic!("{:?}", txn);
    //assert_eq!(wire_txn.len(), 3);
    let tx0 = SanitizedTransaction::from_transaction_for_tests(txn);
    let task = SchedulingStateMachine::create_task(tx0, 0, &mut |_| Page::default());
    let mut scheduler = SchedulingStateMachine::default();
    toggle_collect();
    let task = scheduler.schedule_task(task).unwrap();
    toggle_collect();
    drop(task);
}

#[library_benchmark]
#[bench::min(0)]
#[bench::one(1)]
#[bench::two(2)]
#[bench::three(3)]
#[bench::normal(32)]
#[bench::large(64)]
#[bench::max(128)]
fn bench_drop_task(account_count: usize) {
    toggle_collect();
    let mut accounts = vec![];
    for _ in 0..account_count {
        accounts.push(AccountMeta::new(Keypair::new().pubkey(), true));
    }

    let payer = Keypair::new();
    let memo_ix = Instruction {
        program_id: Pubkey::default(),
        accounts,
        data: vec![0x00],
    };
    let mut ixs = vec![];
    for _ in 0..1 {
        ixs.push(memo_ix.clone());
    }
    let msg = Message::new(&ixs, Some(&payer.pubkey()));
    let txn = Transaction::new_unsigned(msg);
    //panic!("{:?}", txn);
    //assert_eq!(wire_txn.len(), 3);
    let tx0 = SanitizedTransaction::from_transaction_for_tests(txn);
    let task = SchedulingStateMachine::create_task(tx0, 0, &mut |_| Page::default());

    toggle_collect();
    drop(task);
    toggle_collect();
}

#[library_benchmark]
#[bench::min(0)]
#[bench::one(1)]
#[bench::two(2)]
#[bench::three(3)]
#[bench::normal(32)]
#[bench::large(64)]
#[bench::max(128)]
fn bench_schedule_task_conflicting(account_count: usize) {
    toggle_collect();
    let mut accounts = vec![];
    for _ in 0..account_count {
        accounts.push(AccountMeta::new(Keypair::new().pubkey(), true));
    }

    let payer = Keypair::new();
    let memo_ix = Instruction {
        program_id: Pubkey::default(),
        accounts,
        data: vec![0x00],
    };
    let mut ixs = vec![];
    for _ in 0..1 {
        ixs.push(memo_ix.clone());
    }
    let msg = Message::new(&ixs, Some(&payer.pubkey()));
    let txn = Transaction::new_unsigned(msg);
    //panic!("{:?}", txn);
    //assert_eq!(wire_txn.len(), 3);
    let tx0 = SanitizedTransaction::from_transaction_for_tests(txn);
    let task = SchedulingStateMachine::create_task(tx0, 0, |_| &mut Page::default());
    let mut scheduler = SchedulingStateMachine::default();
    let task = scheduler.schedule_task(task).unwrap();
    let task2 = task.clone();
    toggle_collect();
    assert_matches!(scheduler.schedule_task(task2), None);
    toggle_collect();
    drop(task);
}

#[library_benchmark]
#[bench::min(3, 0)]
#[bench::one(3, 1)]
#[bench::two(2, 2)]
#[bench::three(3, 3)]
#[bench::normal(3, 32)]
#[bench::large(3, 64)]
#[bench::large2(3, 128)]
#[bench::large3(3, 256)]
#[bench::large4(3, 1024)]
#[bench::large5(3, 2048)]
fn bench_schedule_task_conflicting_hot(account_count: usize, task_count: usize) {
    toggle_collect();
    let mut accounts = vec![];
    for _ in 0..account_count {
        accounts.push(AccountMeta::new(Keypair::new().pubkey(), true));
    }

    let payer = Keypair::new();
    let memo_ix = Instruction {
        program_id: Pubkey::default(),
        accounts,
        data: vec![0x00],
    };
    let mut ixs = vec![];
    for _ in 0..1 {
        ixs.push(memo_ix.clone());
    }
    let msg = Message::new(&ixs, Some(&payer.pubkey()));
    let txn = Transaction::new_unsigned(msg);
    //panic!("{:?}", txn);
    //assert_eq!(wire_txn.len(), 3);
    let tx0 = SanitizedTransaction::from_transaction_for_tests(txn);

    let mut scheduler = SchedulingStateMachine::default();

    let mut pages: std::collections::HashMap<solana_sdk::pubkey::Pubkey, Page> =
        std::collections::HashMap::new();
    let task = SchedulingStateMachine::create_task(tx0.clone(), 0, &mut |address| {
        pages.entry(address).or_default().clone()
    });
    let task = scheduler.schedule_task(task).unwrap();
    for i in 1..=task_count {
        let task = SchedulingStateMachine::create_task(tx0.clone(), i, &mut |address| {
            pages.entry(address).or_default().clone()
        });
        assert_matches!(scheduler.schedule_task(task), None);
    }

    let task = SchedulingStateMachine::create_task(tx0.clone(), task_count + 1, &mut |address| {
        pages.entry(address).or_default().clone()
    });
    let task2 = task.clone();

    toggle_collect();
    assert_matches!(scheduler.schedule_task(task2), None);
    toggle_collect();

    drop(task);
}

#[library_benchmark]
#[bench::min(0)]
#[bench::one(1)]
#[bench::two(2)]
#[bench::three(3)]
#[bench::normal(32)]
#[bench::large(64)]
#[bench::max(128)]
fn bench_deschedule_task_conflicting(account_count: usize) {
    toggle_collect();
    let mut accounts = vec![];
    for _ in 0..account_count {
        accounts.push(AccountMeta::new(Keypair::new().pubkey(), true));
    }

    let payer = Keypair::new();
    let memo_ix = Instruction {
        program_id: Pubkey::default(),
        accounts,
        data: vec![0x00],
    };
    let mut ixs = vec![];
    for _ in 0..1 {
        ixs.push(memo_ix.clone());
    }
    let msg = Message::new(&ixs, Some(&payer.pubkey()));
    let txn = Transaction::new_unsigned(msg);
    //panic!("{:?}", txn);
    //assert_eq!(wire_txn.len(), 3);
    let tx0 = SanitizedTransaction::from_transaction_for_tests(txn);
    let task = SchedulingStateMachine::create_task(tx0, 0, |_| Page::default());
    let mut scheduler = SchedulingStateMachine::default();
    let task = scheduler.schedule_task(task).unwrap();
    assert_matches!(scheduler.schedule_task(task.clone()), None);

    toggle_collect();
    scheduler.deschedule_task(&task);
    toggle_collect();

    drop(task);
}

#[library_benchmark]
#[bench::min(0)]
#[bench::one(1)]
#[bench::two(2)]
#[bench::three(3)]
#[bench::normal(32)]
#[bench::large(64)]
#[bench::max(128)]
fn bench_schedule_retryable_task(account_count: usize) {
    toggle_collect();
    let mut accounts = vec![];
    for _ in 0..account_count {
        accounts.push(AccountMeta::new(Keypair::new().pubkey(), true));
    }

    let payer = Keypair::new();
    let memo_ix = Instruction {
        program_id: Pubkey::default(),
        accounts,
        data: vec![0x00],
    };
    let mut ixs = vec![];
    for _ in 0..1 {
        ixs.push(memo_ix.clone());
    }
    let msg = Message::new(&ixs, Some(&payer.pubkey()));
    let txn = Transaction::new_unsigned(msg);
    //panic!("{:?}", txn);
    //assert_eq!(wire_txn.len(), 3);
    let tx0 = SanitizedTransaction::from_transaction_for_tests(txn);
    let mut pages: std::collections::HashMap<solana_sdk::pubkey::Pubkey, Page> =
        std::collections::HashMap::new();
    let task = SchedulingStateMachine::create_task(tx0.clone(), 0, |address| {
        pages.entry(address).or_default().clone()
    });
    let task2 = SchedulingStateMachine::create_task(tx0, 1, |address| {
        pages.entry(address).or_default().clone()
    });
    let mut scheduler = SchedulingStateMachine::default();
    let task = scheduler.schedule_task(task).unwrap();
    assert_matches!(scheduler.schedule_task(task2), None);
    scheduler.deschedule_task(&task);
    toggle_collect();
    let retried_task = scheduler.schedule_retryable_task().unwrap();
    toggle_collect();
    assert_eq!(task.transaction(), retried_task.transaction());
    drop(task);
}

#[library_benchmark]
#[bench::min(0)]
#[bench::one(1)]
#[bench::two(2)]
#[bench::three(3)]
#[bench::normal(32)]
#[bench::large(64)]
#[bench::max(128)]
fn bench_deschedule_task(account_count: usize) {
    toggle_collect();
    let mut accounts = vec![];
    for _ in 0..account_count {
        accounts.push(AccountMeta::new(Keypair::new().pubkey(), true));
    }

    let payer = Keypair::new();
    let memo_ix = Instruction {
        program_id: Pubkey::default(),
        accounts,
        data: vec![0x00],
    };
    let mut ixs = vec![];
    for _ in 0..1 {
        ixs.push(memo_ix.clone());
    }
    let msg = Message::new(&ixs, Some(&payer.pubkey()));
    let txn = Transaction::new_unsigned(msg);
    //panic!("{:?}", txn);
    //assert_eq!(wire_txn.len(), 3);
    let tx0 = SanitizedTransaction::from_transaction_for_tests(txn);
    let task = SchedulingStateMachine::create_task(tx0, 0, |_| Page::default());
    let mut scheduler = SchedulingStateMachine::default();
    let task = scheduler.schedule_task(task).unwrap();
    toggle_collect();
    scheduler.deschedule_task(&task);
    toggle_collect();
    drop(task);
}

library_benchmark_group!(
    name = bench_scheduling_state_machine;
    benchmarks = bench_drop_task, bench_schedule_task, bench_schedule_task_conflicting, bench_schedule_task_conflicting_hot, bench_deschedule_task, bench_deschedule_task_conflicting, bench_schedule_retryable_task
);

main!(library_benchmark_groups = bench_scheduling_state_machine);
