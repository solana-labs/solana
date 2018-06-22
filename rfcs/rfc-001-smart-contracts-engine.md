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

 In Figure 1. an untrusted client, creates a program in the front-end language of her choice, (like C/C++/Rust/Lua), and compiles it with LLVM to a position independnet shared object ELF, targeting BPF bytecode. Solana will safely load and execute the ELF.

## Bytecode

Our bytecode is based on Berkley Packet Filter. The requirements for BPF overlap almost exactly with the requirements we have

1. Deterministic amount of time to execute the code
2. Bytecode that is portable between machine instruction sets
3. Verified memory accesses
4. Fast to load the object, verify the bytecode and JIT to local machine instruction set

For 1, that means that loops are unrolled, and for any jumps back we can guard them with a check against the number of instruction that have been executed at this point.  If the limit is reached, the program yields its execution.  This involves saving the stack and current instruction index.

For 2, the BPF bytecode already easily maps to x86–64, arm64 and other instruction sets. 

For 3, every load and store that is relative can be checked to be within the expected memory that is passed into the ELF.  Dynamic load and stores can do a runtime check against available memory, these will be slow and should be avoided.

For 4, Statically linked PIC ELF with just a signle RX segment.  Effectively we are linking a shared object with `-fpic -target bpf` and a linker script to collect everything into a single RX segment.  Writable globals are not supported at the moment.

## Loader
The loader is our first smart contract. The job of this contract is to load the actual program with its own instance data.  The loader expects the shared object to implement the following methods:
```
void map(const struct module_data *module_data, struct transaction* tx, uint8_t *scratch);

void reduce(
    const struct module_data *module_data,
    const transaction *txs,
    uint32_t num,
    const struct reduce* reductions,
    uint32_t num_rs,
    struct reduce* reduced
);

void finalize(
    const struct module_data *module_data,
    const transaction *txs,
    uint32_t num,
    struct reduce* reduce
);
```
The module_data structure is configued by the client, it contains the `struct solana_module` structure at the top, which defines how to calculate how much buffer to provide for each step.

A client will create a transaction to create a new loader instance:

`Solana_NewLoader(Loader instance PubKey, proof of key ownership, space I need for my elf)`

A client will then do a bunch of transactions to load its elf into the loader instance they created:

`Loader_UploadElf(Loader instance PubKey, proof of key ownership, pos start, pos end, data)`

`Loader_NewInstance(Loader instance PubKey, proof of key ownership, Instance PubKey, proof of key owndership)`

A client will then do a bunch of transactions to load its elf into the loader instance they created:

`Instance_UploadModuleData(Instance PubKey, proof of key ownership, pos start, pos end, data)`

```
struct module_hdr {
    struct pubkey owner;
    uint32_t map_scratch_size;
    uint32_t map_data_size;
    uint32_t reduce_size;
    uint32_t reduce_scratch_size;
    uint32_t finalize_scratch_size;
};
```

At this point the client may need to upload more R user data to the OS via some more transactions to the loader:

`Instance_Start(Instance PubKey, proof of key owndership)`

At this point clients can start sending transactions to the instance

## Parallelizable Runtime

To parallelize smart contract execution we plan on breaking up contracts into distinct interfaces, Map/Collect/Reduce/Finalize.

### Map and Collect

```
struct transaction {
   struct transaction_msg msg;
   uint8_t favorite;
}
struct module_data {
   struct module_hdr hdr;
}
void map(const struct module_data *module_data, struct transaction* tx, uint8_t *scratch)
{
    //msg.userdata is a network protocol defined fixed size that is an input from the user via the transaction
    tx->favorite = tx->msg.userdata[0];
    collect(&tx->hdr);
}
```

The contract's object file implements a map function and lays out memory that is allocated per transaction. It then tells the runtime to collect this transaction for further processing if it's accepted by the contract. The mapped memory is stored as part of the transaction, and only transactions that succeed in a `collect` call will get accepted by this contract and move to the next stage.

### Reduce

```
struct reduce {
    struct reduce_hdr hdr;
    uint64_t votes[256];
}
void reduce(
    const struct module_data *module_data,
    const transaction *txs,
    uint32_t num,
    const struct reduce* reductions,
    uint32_t num_rs,
    struct reduce* reduced
) {
    struct reduce *reduced = (struct reduce*)scratch;
    int i = 0;
    for(int i = 0; i < num; ++i) {
        struct Vote *v = collected(&txs[i]);
        reduced->votes[txs[i].favorite] += txs[i].msg.amount;
    }
    for(int i = 0; i < num_rs; ++i) {
        for(j = 0; j < 256; ++j) {
            reduced->votes[j] += reductions[i].votes[j];
        }
    }
}
```
Reduce allows the contract to accumilate all the `collect` and `reduce` calls into a single structure.

### Finalize

Finalize is then called when some final condition occurs. This could be when the time expires on the contract, or from a direct call to finalize itself, such as finalize(reduce). 

```
void finalize(
    const struct module_data *module_data,
    const transaction *txs,
    uint32_t num,
    struct reduce* reduce
) {
    int i, s = 0;
    uint64_t total = 0;
    uint8_t max = 0;
    for(i = 0; i < 256; ++i) {
        if reduce->votes[max] < reduce->votes[i] {
            max = i;
        }
        total += reduce->votes[i];
    }
    //now we have to spend the transactions
    for(i = 0; i < num; ++i) {
        struct transaction *dst = &txs[i];
        if txs[i]->favorite != max {
            continue;
        }
        uint64_t award = total * dst.hdr->amount / reduced->votes[max];
        for(; s < num; ++s) {
            struct transaction *src = &txs[s];
            if src->favorite == max {
                continue;
            }
            uint64_t amt = MIN(src->hdr.amount, award);
            //mark the src transaction as spent
            spend(&src->hdr, amt, dst.hdr.from);
            award -= amt;
            if award == 0 {
                break;
            }
        }
    }
    //spend the rounding errors on myself
    for(; s < num; ++s) {
        struct transaction *src = &txs[s];
        spend(&src->hdr, src->hdr.amount, module_data->hdr.owner);
    }
}
```

## Notes

1. There is no dynamic memory allocation.
2. Transactions are tracked by the runtime and not the contract
3. Transactions must be spent, if they are not spent the runtime can cancel and refund them minus fees
