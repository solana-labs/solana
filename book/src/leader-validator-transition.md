# Leader and Validator Transition

A fullnode operates in two modes, leader and validator.  The modes overlap in code and in function, but have different behaviors.  The goal for this design is to allow the two modes to transition between states while reusing the computed bank state.


## Validator

A validator operates on may different concurrent forks of the bank state until it can prove a PoH record at a height that is the scheduled slot to be a leader.

## Leader

A leader operates on a single fork for a specific PoH slot.


## PoH

Two different objects for managing PoH for leaders and validators.  Both only work for a specific range of ticks and will error out and send an exit signal when they are done.

### PoH Generator

This object handles PoH generation for a Validator. 

```
PohGenerator {
    /// Start the PoH thread, and compute it until end_ticks.
    /// Once computed, the exit_signal is sent
    pub fn new(start: &Hash, start_ticks: u64, end_ticks: u64, exit_signal: Sender<()>);

    /// The is called when a validator votes and needs to reset PoH with its vote
    /// Error is returned if end_ticks is reached first
    pub fn reset(last_id: &Hash, reset_height: u64) -> Result<()>;

    /// The list of entries from `start` or `reset_height` to `end_ticks`
    /// Error is returned if end_ticks has not been reached yet
    pub fn final_entries() -> Result<Vec<Entry>>;
}
```

### PoH Recorder

This object handles PoH recording for a Leader
 ```
PohRecorder {
    /// Start the PoH thread, and compute it until end_ticks.
    /// The recorder will continue to call register tick on the BankState (TODO: we can remove this if the slot ticks are the only valid ticks)
    /// Once computed, the exit_signal is sent
    pub fn new(
        entries: &[Entry],
        end_ticks: u64,
        sender: Sender<Vec<Entry>>,
        bank_state: Arc<BankState>, exit_signal: Sender<()>);

    /// Record transactions
    /// Error is returned if end_ticks is reached first
    pub fn record(&self, mixin: Hash, txs: Vec<Transaction>) -> Result<()>;
}
```


## Fullnode Loop

This object manages the transition between modes.  The main idea is that once a ledger is replayed, the validator can run until there is proof that it can be a leader.  The leader can then run and record its transactions.

```
let (exit_receiver, exit_signal) = channel();
loop {
    // run the validator loop first
    let my_slot = leader_scheduler.get_scheduled_tick_height(my_id);
    let last_fork = forks.get_latest_fork();

    // the generator will exit immediatly if tick_height == my_slot.start 
    assert!(last_fork.tick_height <= my_slot.start, "start is out of sync, replay the ledger!");
    let generator = PohGenerator::new(last_fork.id, last_fork.tick_height, my_slot.start, exit_signal);

    // operate the validator, this runs until my_slot.start is reached by the generator
    let validator = Validator::new(forks, bank, generator);

    // wait for validator to finish
    exit_receiver.recv();
    validator.exit(). 
    
    // these entries connect to my_slot
    let poh_entries = generator.entries();

    // make sure
    assert!(poh_entries.last().unwrap().tick_height == my_slot.start, "generator didn't reach my scheduled height, abort!");

    // operate as leader
    let bank_state = forks.get(my_slot.fork_id);
    let recorder = PohRecorder::new(poh_entries, my_slot.end, bank_state, exit_signal);
    let leader = Leader::new(bank_state); 
    exit_receiver.recv();
    // finalize the bank_state and send the vote as a validator
    leader.exit();
}
```
