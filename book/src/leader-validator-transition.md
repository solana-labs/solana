# Leader and Validator Transition

A fullnode operates in two modes, leader and validator.  The modes overlap in
code and in function, but have different behaviors.  The goal for this design is
to allow the two modes to transition between states while reusing the computed
bank state.


## Validator

A validator operates on may different concurrent forks of the bank state until
it can prove a PoH record at a height that is the scheduled slot to be a leader.

## Leader

A leader operates on a single fork for a specific PoH slot.


## PoH

Two different objects for managing PoH for leaders and validators.  Both only
work for a specific range of ticks and will error out and send an exit signal
when they are done.

### PoH Generator

This object handles PoH generation for a Validator.  It solves the following
problems for validators

1. Keep track of *time* as defined by PoH height, and stop the validator when it
hits the scheduled leader slot.

2. Reset the clock whenever a validator votes.  The last validator vote is the
validators most recent commitment. It is therefore the closest fork to the
scheduled slot.

3. Provide the entries that connect the last vote to the scheduled slot.

``` PohGenerator {
    /// Start the PoH thread, and compute it until end_ticks.
    /// Once computed, the exit_signal is sent
    pub fn new(
        start: &Hash,
        start_ticks: u64,
        end_ticks: u64,
        exit_signal: Sender<()>);

    /// The is called when a validator votes and needs to reset PoH with its
    /// vote.  Error is returned if end_ticks is reached first.
    pub fn reset(last_id: &Hash, reset_height: u64) -> Result<()>;

    /// The list of entries from `start` or `reset_height` to `end_ticks`.
    /// Error is returned if end_ticks has not been reached yet.
    pub fn final_entries() -> Result<Vec<Entry>>;
} ```

### PoH Recorder

This object handles PoH recording for a Leader.  It solves the following
problems:

1. Keep track of *time* as defined by PoH height, and stop the leader when it
gets to the end of its scheduled leader slot.

2. Register ticks with the BankState as they are produced by PoH.

3. Record entries that are produced by the TPU into PoH.

```
PohRecorder {
    /// Start the PoH thread, and compute it until end_ticks.
    /// The recorder will continue to call register tick on the BankState.
    /// TODO: we can remove `register_ticks` on the bank if the slot ticks are
    /// the only valid ticks.
    /// Once computed, the exit_signal is sent.
    pub fn new(
	    entries: &[Entry],
        end_ticks: u64,
        sender: Sender<Vec<Entry>>,
	    bank_state: Arc<BankState>,
        exit_signal: Sender<()>);

    /// Record transactions /// Error is returned if end_ticks is reached first
    pub fn record(&self, mixin: Hash, txs: Vec<Transaction>) -> Result<()>;
} 
```


## Fullnode Loop

This object manages the transition between modes.  The main idea is that once a
ledger is replayed, the validator can run until there is proof that it can be a
leader.  The Leader can then run and record its transactions.

``` 
let (exit_receiver, exit_signal) = channel(); 
loop {
    // run the validator loop first
    let my_slot = leader_scheduler.get_scheduled_tick_height(my_id);
    let last_fork = forks.get_latest_fork();

    // the generator will exit immediatly if tick_height == my_slot.start
    assert!(last_fork.tick_height <= my_slot.start,
        "start is out of sync, replay the ledger!");
    let generator = PohGenerator::new(last_fork.id, last_fork.tick_height, my_slot.start, exit_signal);

    // operate the validator, this runs until my_slot.start is reached by the
    // generator
    let validator = Validator::new(forks, bank, generator, //other params);

    // wait for validator to finish exit_receiver.recv(); validator.exit().

    // these entries connect to my_slot
    let poh_entries = generator.entries();

    // make sure this is true
    assert!(poh_entries.last().unwrap().tick_height == my_slot.start,
        "generator didn't reach my scheduled height, abort!");

    // get the slot these entries connect to
    let starting_slot = poh_entries.first().unwrap().slot_index() - 1;

    // create a fork from the start to my slot
    let bank_state = forks.init(my_slot.slot_id, starting_slot);

    // operate as leader
    let recorder = PohRecorder::new(poh_entries, my_slot.end, bank_state, exit_signal);
    let leader = Leader::new(bank_state, //other params);
    exit_receiver.recv(); leader.exit();

    // Finalize the bank_state, send the vote as a validator.
    bank_state.finalize();
}
```

## BankState

BankState operates over a specific slot.  Once the final tick has been
registered the state becomes finalized and further writes will error out.
Validators operate over a bunch of different BankStates that represent live
active chains.  A Leader only operates over a single BankState.
