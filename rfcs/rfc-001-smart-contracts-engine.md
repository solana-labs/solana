# Smart Contracts Engine

Our approach to smart contract execution is based on how operating systems load and execute dynamic code in the kernel. 

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
     |      userspace      |       |       kernel        |
     +---------------------+       +---------------------+


[Figure 1. Smart Contracts Stack]

 In Figure 1. an untrusted client, or `Userspace` in Operating Systems terms, creates a program in the front-end language of her choice, (like C/C++/Rust/Lua), and compiles it with LLVM to the Solana Bytecode object. This object file is a standard ELF. We use the section headers in the ELF format to annotate memory in the object such that the Kernel aka the Solana blockchain, can safely and efficiently load it for execution.

The computationally expensive work of converting frontend languages into programs is done locally by the client. The output is a ELF with specific section headers and with a bytecode as its target that is designed for quick verification and conversion to the local machine instruction set that Solana is running on.

## Solana Bytecode

Our bytecode is based on Berkley Packet Filter. The requirements for BPF overlap almost exactly with the requirements we have

1. Deterministic amount of time to execute the code
2. Bytecode that is portable between machine instruction sets
3. Verified memory accesses
4. Fast to load the object, verify the bytecode and JIT to local machine instruction set

For 1, we can unroll the loops, and for any jumps back we can guard them with a check against the number of instruction that have been executed at this point.  If the limit is reached, the program yields.  This involves saving the stack and current instruction index to the RW segment of the elf.

For 2, the BPF bytecode already easily maps to x86–64, arm64 and other instruction sets. 

For 3, every load and store that is PC relative can be checked to be within the ELF.  Dynamic load and stores can dynamically guard against load and stores to dynamic memory.
For 4, Statically linked elf with just a signle R/RX/RW segments.  Effectively we are linking with `-static --nostd -target bpf` and a linker script to collect everything into a single spot.  The R segment is for read only instance data that is populated by the loader.

## Loader
The loader is our first smart contract. The job of this contract is to load the actual program with its own instance data. 

       +----------------------+
       |                      |
       |  +----------------+  |
       |  |    RX-code     |  |
       |  +----------------+  |
       |                      |
       |  +----------------+  |
       |  |     R-data     |  |
       |  +----------------+  |
       |                      |
       |  +----------------+  |
       |  |    RW-data     |  |
       |  +----------------+  |
       |         elf          |
       +----------------------+

A client will create a transaction to create a new loader instance.
* `NewLoaderAtPubKey(Loader instance PubKey, proof of key ownership, space i need for my elf)`

A client will then do a bunch of transactions to load its elf into the loader instance they created.

* `LoaderConfigureInstance(Loader instance PubKey, proof of key ownership, amount of space I need for R user data, user data)`

At this point the client may need to upload more R user data to the OS via some more transactions to the loader.

* `LoaderStart(Loader instance PubKey, proof of key owndership)`

At htis point clients can start sending transactions to the instance

## Parallelizable Runtime
To parallelize smart contract execution we plan on breaking up contracts into distinct sections, Map/Collect/Reduce/Finalize. These distinct sections are the interface between the ELF and the Kernel that is executing it. Memory is loaded into the symbols defined in these sections, and relevant constants are set.
```
struct Vote {
   Address from;
   uint64_t amount;
   uint8_t favorite;
}
struct Vote vote;
void map(struct Transaction *tx)
{
    memmove(&vote.from, &tx.from, sizeof(vote.from));
    vote.amount = tx.amount;
    vote.favorite = tx.userdata[0];
    collect(&vote, sizeof(vote));
}
```
The contract's object file implements a map function and lays out memory that is allocated per transaction. It then yields itself to a collect call that is schedule to run sometime later.
```
void collect(void* data[], uint32_t sizes[], uint32_t num)
{
   used = sizeof(struct Vote) * votelen;
   memmove(totals, vote, used);
   reduce)
}
```
Reduce
```
void reduce()
{
   int i;
   for(i = 0; i < len; ++i) {
      memmove(&totals[used/sizeof(struct Vote)], vote[i], sizes[i]);
      used += sizes[i];
   }
   for(i = 0; i < rlen; ++i) {
      memmove(&totals[used/sizeof(struct Vote)], rtotals[i], rsizes[i]);
      used += rsizes[i];
   }
}
```
finalize is then called when some final condition occurs. This could be when the time expires on the contract, or from a direct call to finalize itself, such as yield(finalize). 
```
void finalize() {
   int i;
   uint64_t total = 0;
   uint8_t max = 0;
   uint32_t num = used/sizeof(struct Vote);
   //scan all the votes and find out which uint8_t is the favorite
   for(i = 0; i < num; ++i) {
      struct Vote *v = &totals[i];
      votes[v->favorite] += v->amount;
      if votes[max] < votes[v->favorite] {
          max = v->favorite
      }
      total += v->amount;
   }
   for(i = 0; i < num; ++i) {
      struct Vote *v = &totals[i];
      if v->favorite == max {
          payout(v->from, total * v->amount / votes[max]);
      }
   }
}
```
## Notes
1. There is no dynamic memory allocation. 
2. You need to derive the maximum amount of memory you need based on the number of user inputs flowing through the contract.
3. You need to tell the engine how much you are actually using so it saves that amount as persistent.
4. Loops need to be unrolled, and the contract will need to yield back to itself with the loop context saved. This means the stack for each function is something we load/store just like any other memory defined in the ELF
Other information about the kernel (current hash, contract address and balance, etc…) can be mapped into the elf as well.
5. We can map more complex data structured as well, like maps and hashtables, lists etc…
6. We can map instance configurable data as another section. So contracts can configure and create themselves.
