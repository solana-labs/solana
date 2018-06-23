# Smart Contracts Engine

The goal of this RFC is to define a set of constraints for APIs and runtime such that we can safely execute our smart contracts safely on massively parallel hardware such as a GPU.

## Toolchain Stack

     +---------------------+       +---------------------+
     |                     |       |                     |
     |   +------------+    |       |   +------------+    |
     |   |            |    |       |   |            |    |
     |   |  frontend  |    |       |   |  verifier  |    |
     |   |            |    |       |   |            |    |
     |   +-----+------+    |       |   +-----+------+    |
     |         |           |       |         |           |
     |         |           |       |         |           |
     |   +-----+------+    |       |   +-----+------+    |
     |   |            |    |       |   |            |    |
     |   |    llvm    |    |       |   |   loader   |    |
     |   |            |    +------>+   |            |    |
     |   +-----+------+    |       |   +-----+------+    |
     |         |           |       |         |           |
     |         |           |       |         |           |
     |   +-----+------+    |       |   +-----+------+    |
     |   |            |    |       |   |            |    |
     |   |    ELF     |    |       |   |   runtime  |    |
     |   |            |    |       |   |            |    |
     |   +------------+    |       |   +------------+    |
     |                     |       |                     |
     |        client       |       |       solana        |
     +---------------------+       +---------------------+

                [Figure 1. Smart Contracts Stack]

In Figure 1. an untrusted client, creates a program in the front-end language of her choice, (like C/C++/Rust/Lua), and compiles it with LLVM to a position independent shared object ELF, targeting BPF bytecode. Solana will safely load and execute the ELF.

## Bytecode

Our bytecode is based on Berkley Packet Filter. The requirements for BPF overlap almost exactly with the requirements we have:

1. Deterministic amount of time to execute the code
2. Bytecode that is portable between machine instruction sets
3. Verified memory accesses
4. Fast to load the object, verify the bytecode and JIT to local machine instruction set

For 1, that means that loops are unrolled, and for any jumps back we can guard them with a check against the number of instruction that have been executed at this point.  If the limit is reached, the program yields its execution.  This involves saving the stack and current instruction index.

For 2, the BPF bytecode already easily maps to x86–64, arm64 and other instruction sets. 

For 3, every load and store that is relative can be checked to be within the expected memory that is passed into the ELF.  Dynamic load and stores can do a runtime check against available memory, these will be slow and should be avoided.

For 4, Fully linked PIC ELF with just a single RX segment. Effectively we are linking a shared object with `-fpic -target bpf` and with a linker script to collect everything into a single RX segment. Writable globals are not supported.

## Loader
The loader is our first smart contract. The job of this contract is to load the actual program with its own instance data.  The loader will verify the bytecode and that the object implements the expected entry points.

Since there is only one RX segment, the context for the contract instance is passed into each entry point as well as the event data for that entry point.

A client will create a transaction to create a new loader instance:

`Solana_NewLoader(Loader Instance PubKey, proof of key ownership, space I need for my elf)`

A client will then do a bunch of transactions to load its elf into the loader instance they created:

`Loader_UploadElf(Loader Instance PubKey, proof of key ownership, pos start, pos end, data)`

At this point the client can create a new instance of the module with its own instance address:

`Loader_NewInstance(Loader Instance PubKey, proof of key ownership, Instance PubKey, proof of key ownership)`

Once the instance has been created, the client may need to upload more user data to solana to configure this instance:

`Instance_UploadModuleData(Instance PubKey, proof of key ownership, pos start, pos end, data)`

Now clients can `start` the instance:

`Instance_Start(Instance PubKey, proof of key ownership)`

## Runtime

Our goal with the runtime is to have a general purpose execution environment that is highly parallelizable and doesn't require dynamic resource management.  Basically, we want to execute as many contracts as we can in parallel, and have them pass or fail without a destructive state change.

### State and Entry Point

State is addressed by an account which is at the moment simply the PubKey.  Our goal is to eliminate dynamic memory allocation in the smart contract itself, so the contract is a function that takes a mapping of [(PubKey,State)] and returns [(PubKey, State')].  The output of keys is a subset of the input.  Three basic kinds of state exist:

* Instance State
* Participant State
* Caller State

There isn't any difference in how each is implemented, but conceptually Participant State is memory that is allocated for each participant in the contract.  Instance State is memory that is allocated for the contract itself, and Caller State is memory that the transactions caller has allocated.


### Call

```
void call(
    const struct instance_data *data,
    const uint8_t kind[],  //instance|participant|caller|read|write
    const uint8_t *keys[],
    uint8_t *data[],
    int num,
    uint8_t dirty[],        //dirty memory bits
    uint8_t *userdata,      //current transaction data
);
```

To call this operation, the transaction that is destined to the contract instance specifies what keyed state it should present to the `call` function.  To allocate the state memory, the client has to first call a function on the contract with the designed address that will own the state.

* `Instance_Allocate(Instance PubKey, My PubKey, Proof of key ownership)`

Any transaction can then call `call` on the contract with a set of keys.  It's up to the contract itself to manage owndership:

* `Instance_Call(Instance PubKey, [Input PubKeys], proofs of ownership, userdata...)`

The contract has read/write privileges to all the memory that is allocated.

### Reduce

Some operations on the contract will require iteration over all the keys.  To make this parallelizable the iteration is broken up into reduce calls which are combined.

```
void reduce_m(
    const struct instance_data *data,
    const uint8_t *keys[],
    const uint8_t *data[],
    int num,
    uint8_t *reduce_data,
);

void reduce_r(
    const struct instance_data *data,
    const uint8_t *reduce_data[],
    int num,
    uint8_t *reduce_data,
);
```

### Execution

Transactions are batched and processed in parallel at each stage.
```
+-----------+    +--------------+      +-----------+    +---------------+
| sigverify |-+->| debit commit |---+->| execution |-+->| memory commit |
+-----------+ |  +--------------+   |  +-----------+ |  +---------------+
              |                     |                |
              |  +---------------+  |                |  +--------------+
              |->| memory verify |->+                +->| debit undo   |
                 +---------------+                   |  +--------------+
                                                     |
                                                     |  +---------------+
                                                     +->| credit commit |
                                                        +---------------+


```
The `debit verify` stage is very similar to `memory verify`.  Proof of key ownership is used to check if the callers key has some state allocated with the contract, then the memory is loaded and executed.  After execution stage, the dirty pages are written back by the contract.  Because know all the memory accesses during execution, we can batch transactions that do not interfere with each other.  We can also apply the `debit undo` and `credit commit` stages of the transaction.  `debit undo` is run in case of an exception during contract execution, only transfers may be reversed, fees are commited to solana.

### GPU execution

A single contract can read and write to separate key pairs without interference.  These separate calls to the same contract can execute on the same GPU thread over different memory using different SIMD lanes.

## Notes

1. There is no dynamic memory allocation.
2. Persistant Memory is allocated to a Key with ownership
3. Contracts can `call` to update key owned state
4. Contracts can `reduce` over the memory to aggregate state
