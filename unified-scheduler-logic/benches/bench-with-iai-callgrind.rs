#![allow(clippy::arithmetic_side_effects)]

#[global_allocator]
static GLOBAL: B = B;

struct A<T>(T);

unsafe impl<T> std::marker::Sync for A<T> {}

static LOCAL_ALLOCATOR: A<std::cell::UnsafeCell<BL>> = A(std::cell::UnsafeCell::new(BL::new()));

struct BL {
    cursor: *mut u8,
    limit: *mut u8,
    bytes: [u8; Self::BLOCK_SIZE],
}

impl BL {
    const BLOCK_SIZE: usize = 100_000_000;

    const fn new() -> Self {
        Self {
            cursor: usize::max_value() as _,
            limit: usize::max_value() as _,
            bytes: [0; Self::BLOCK_SIZE],
        }
    }

    #[inline(always)]
    pub fn alloc2(&mut self, bytes: usize) -> *mut u8 {
        loop {
            self.cursor = unsafe { (((self.cursor.sub(bytes)) as usize) & !15) as _ };
            if self.cursor >= self.limit {
                return self.cursor;
            } else if self.limit == usize::max_value() as _ {
                self.limit = self.bytes.as_mut_ptr();
                self.cursor = unsafe { self.limit.add(Self::BLOCK_SIZE) };
                continue;
            } else {
                panic!("out of memory form BL");
            }
        }
    }
}

use std::{
    alloc::{GlobalAlloc, Layout},
    hint::black_box,
};

struct B;

unsafe impl GlobalAlloc for B {
    #[inline(always)]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        (*LOCAL_ALLOCATOR.0.get()).alloc2(layout.size())
    }

    #[inline(always)]
    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}

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
        transaction::{SanitizedTransaction, Transaction},
    },
    solana_unified_scheduler_logic::{Page, SchedulingStateMachine},
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
    for i in 0..account_count {
        if i % 2 == 0 {
            accounts.push(AccountMeta::new(Keypair::new().pubkey(), true));
        } else {
            accounts.push(AccountMeta::new_readonly(Keypair::new().pubkey(), true));
        }
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
    let mut scheduler =
        unsafe { SchedulingStateMachine::exclusively_initialize_current_thread_for_scheduling() };
    toggle_collect();
    let task = scheduler.schedule_task(task);
    toggle_collect();
    task.unwrap();
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
#[bench::one(1)]
fn bench_insert_task(account_count: usize) {
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

    let mut b = std::collections::BTreeMap::new();
    toggle_collect();
    b.insert(task.index, task.clone());
    b.insert(task.index + 1, task.clone());
    b.remove(&task.index);
    b.remove(&(task.index + 1));
    //b.insert(task.index + 4, task);
    toggle_collect();
    drop(b);
}

#[library_benchmark]
#[bench::arc_new(1)]
#[bench::arc_new_and_clone(2)]
#[bench::rc_new(3)]
#[bench::rc_new_and_clone(4)]
fn bench_arc(account_count: usize) {
    toggle_collect();

    {
        let b;
        match account_count {
            1 => {
                toggle_collect();
                b = black_box(std::sync::Arc::new(black_box(3_u32)));
            }
            2 => {
                b = black_box(std::sync::Arc::new(black_box(3_u32)));
                toggle_collect();
                std::mem::forget(black_box(b.clone()));
            }
            _ => {
                let b;
                match account_count {
                    3 => {
                        toggle_collect();
                        b = black_box(std::rc::Rc::new(black_box(3_u32)));
                    }
                    4 => {
                        toggle_collect();
                        b = black_box(std::rc::Rc::new(black_box(3_u32)));
                        black_box(b.clone());
                    }
                    _ => panic!(),
                }
                toggle_collect();
                drop(b);
                return;
            }
        }
        toggle_collect();
        drop(b);
    }
}

#[library_benchmark]
#[bench::arc_new(1)]
#[bench::arc_new_and_clone(2)]
#[bench::rc_new(3)]
#[bench::rc_new_and_clone(4)]
fn bench_triomphe_arc(account_count: usize) {
    toggle_collect();

    {
        let b;
        match account_count {
            1 => {
                toggle_collect();
                b = black_box(triomphe::Arc::new(black_box(3_u32)));
            }
            2 => {
                b = black_box(triomphe::Arc::new(black_box(3_u32)));
                toggle_collect();
                std::mem::forget(black_box(b.clone()));
            }
            _ => {
                let b;
                match account_count {
                    3 => {
                        toggle_collect();
                        b = black_box(std::rc::Rc::new(black_box(3_u32)));
                    }
                    4 => {
                        toggle_collect();
                        b = black_box(std::rc::Rc::new(black_box(3_u32)));
                        black_box(b.clone());
                    }
                    _ => panic!(),
                }
                toggle_collect();
                drop(b);
                return;
            }
        }
        toggle_collect();
        drop(b);
    }
}

#[library_benchmark]
#[bench::one(1)]
fn bench_heaviest_task(account_count: usize) {
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

    let mut b = std::collections::BTreeMap::new();
    b.insert(task.index, task.clone());
    b.insert(task.index + 1, task.clone());
    b.insert(task.index + 2, task.clone());
    let mut c = std::collections::BTreeMap::new();
    c.insert(task.index + 3, task.clone());
    c.insert(task.index + 4, task.clone());
    c.insert(task.index + 5, task.clone());

    toggle_collect();
    let d = b.first_key_value();
    let e = c.first_key_value();
    let f = std::cmp::min_by(d, e, |x, y| x.map(|x| x.0).cmp(&y.map(|y| y.0))).map(|x| x.1);
    assert_matches!(f.map(|f| f.task_index()), Some(0));
    toggle_collect();
    dbg!(f);

    drop(b);
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
    let task = SchedulingStateMachine::create_task(tx0, 0, &mut |_| Page::default());
    let mut scheduler =
        unsafe { SchedulingStateMachine::exclusively_initialize_current_thread_for_scheduling() };
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

    let mut scheduler =
        unsafe { SchedulingStateMachine::exclusively_initialize_current_thread_for_scheduling() };

    let mut pages: std::collections::HashMap<solana_sdk::pubkey::Pubkey, Page> =
        std::collections::HashMap::new();
    let task = SchedulingStateMachine::create_task(tx0.clone(), 0, &mut |address| {
        pages.entry(address).or_default().clone()
    });
    scheduler.schedule_task(task).unwrap();
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
    let task = SchedulingStateMachine::create_task(tx0, 0, &mut |_| Page::default());
    let mut scheduler =
        unsafe { SchedulingStateMachine::exclusively_initialize_current_thread_for_scheduling() };
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
fn bench_schedule_unblocked_task(account_count: usize) {
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
    let task = SchedulingStateMachine::create_task(tx0.clone(), 0, &mut |address| {
        pages.entry(address).or_default().clone()
    });
    let task2 = SchedulingStateMachine::create_task(tx0, 1, &mut |address| {
        pages.entry(address).or_default().clone()
    });
    let mut scheduler =
        unsafe { SchedulingStateMachine::exclusively_initialize_current_thread_for_scheduling() };
    let task = scheduler.schedule_task(task).unwrap();
    assert_matches!(scheduler.schedule_task(task2), None);
    scheduler.deschedule_task(&task);
    toggle_collect();
    let retried_task = scheduler.schedule_unblocked_task();
    toggle_collect();
    let retried_task = retried_task.unwrap();
    assert_eq!(task.transaction(), retried_task.transaction());
    drop(task);
}

#[library_benchmark]
#[bench::min(0)]
#[bench::one(1)]
#[bench::two(2)]
#[bench::three(3)]
#[bench::small(16)]
#[bench::normal(32)]
#[bench::large(64)]
//#[bench::max(128)]
fn bench_end_to_end_worst(account_count: usize) {
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
    let task = SchedulingStateMachine::create_task(tx0.clone(), 0, &mut |address| {
        pages.entry(address).or_default().clone()
    });
    let mut scheduler =
        unsafe { SchedulingStateMachine::exclusively_initialize_current_thread_for_scheduling() };

    let task = scheduler.schedule_task(task).unwrap();
    for i in 1..account_count {
        let mut accounts = vec![memo_ix.accounts[i].clone()];
        //let mut accounts = vec![AccountMeta::new(Keypair::new().pubkey(), true)];
        for _ in 0..account_count {
            accounts.push(AccountMeta::new(Keypair::new().pubkey(), true));
        }

        let payer = Keypair::new();
        let memo_ix = Instruction {
            program_id: Pubkey::default(),
            accounts,
            data: vec![0x00],
        };
        let ixs = vec![memo_ix];
        let msg = Message::new(&ixs, Some(&payer.pubkey()));
        let txn = Transaction::new_unsigned(msg);
        //panic!("{:?}", txn);
        //assert_eq!(wire_txn.len(), 3);
        let tx0 = SanitizedTransaction::from_transaction_for_tests(txn);
        let task2 = SchedulingStateMachine::create_task(tx0, i, &mut |address| {
            pages.entry(address).or_default().clone()
        });
        toggle_collect();
        let scheduled_task = scheduler.schedule_task(task2.clone());
        toggle_collect();
        drop(scheduled_task);
    }

    toggle_collect();
    scheduler.deschedule_task(&task);
    if let Some(_cc) = account_count.checked_sub(1) {
        //assert_eq!(scheduler.unblocked_task_count(), cc);
        //let mut c = 0;
        while let Some(retried_task) = scheduler.schedule_unblocked_task() {
            //c += 1;
            //scheduler.deschedule_task(&retried_task);
            toggle_collect();
            drop::<solana_unified_scheduler_logic::Task>(retried_task);
            toggle_collect();
        }
        //assert_eq!(c, cc);
    }
    toggle_collect();

    //assert_eq!(task2.task_index(), retried_task.task_index());
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
    for i in 0..account_count {
        if i % 2 == 0 {
            accounts.push(AccountMeta::new(Keypair::new().pubkey(), true));
        } else {
            accounts.push(AccountMeta::new_readonly(Keypair::new().pubkey(), true));
        }
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
    let mut scheduler =
        unsafe { SchedulingStateMachine::exclusively_initialize_current_thread_for_scheduling() };
    let task = scheduler.schedule_task(task).unwrap();
    toggle_collect();
    scheduler.deschedule_task(&task);
    toggle_collect();
    drop(task);
}

library_benchmark_group!(
    name = bench_scheduling_state_machine;
    benchmarks = bench_end_to_end_worst, bench_arc, bench_triomphe_arc, bench_drop_task, bench_insert_task, bench_heaviest_task, bench_schedule_task, bench_schedule_task_conflicting, bench_schedule_task_conflicting_hot, bench_deschedule_task, bench_deschedule_task_conflicting, bench_schedule_unblocked_task
    //benchmarks = bench_arc, bench_triomphe_arc
    //benchmarks = bench_end_to_end_worst
);

main!(library_benchmark_groups = bench_scheduling_state_machine);
