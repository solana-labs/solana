# Smart Contracts Engine

Our approach to smart contract execution is based on how operating systems load and execute dynamic code in the kernel. 

![Figure 1. Smart Contracts Stack](images/smart-contracts-stack.png)

In Figure 1. an untrusted client, or *Userspace* in Operating Systems terms, creates a program in the front-end language of her choice, (like C/C++/Rust/Lua), and compiles it with LLVM to the Solana Bytecode object. This object file is a standard ELF. We use the section headers in the ELF format to annotate memory in the object such that the Kernel aka the Solana blockchain, can safely and efficiently load it for execution.

The computationally expensive work of converting frontend languages into programs is done locally by the client. The output is a ELF with specific section headers and with a bytecode as its target that is designed for quick verification and conversion to the local machine instruction set that Solana is running on.

## Solana Bytecode

Our bytecode is based on Berkley Packet Filter. The requirements for BPF overlap almost exactly with the requirements we have

1. Deterministic amount of time to execute the code
2. Bytecode that is portable between machine instruction sets
3. Verified memory accesses
4. Fast to load the object, verify the bytecode and JIT to local machine instruction set

For 1 BPF toolchain is designed for generating code without jumps back. For us this means loops are unrolled, and the stack itself is part of the ELF. Any work that we do here to expand how the kernel can analyze the runtime of the generated code can be ported back to the Linux community.

For 2, the bytecode already easily maps to x86–64, arm64 and other instruction sets. 

For 3 and 4, is in a single pass we want to check that all the load and stores are pointing to the memory defined in the ELF,and map all the instructions to x86 or SPIR-V, or some other local machine instruction set. Linux already ships with a BPF JIT, but we love Rust, so would likely reimplemnt it in rust.

## Loader aka Dynamic Linker
The loader is our first smart contract. The job of this contract is to load the actual program with its own instance data.

## Parallelizable Runtime
To parallelize smart contract execution we plan on breaking up contracts into distinct sections, Map/Collect/Reduce/Finalize. These distinct sections are the interface between the ELF and the Kernel that is executing it. Memory is loaded into the symbols defined in these sections, and relevant constants are set.
```
struct Vote {
   Address from;
   uint64_t amount;
   uint8_t favorite;
}
__section("map.data")
struct Vote vote;
__section("map")
void map(struct Transaction *tx)
{
    memmove(&vote.from, &tx.from, sizeof(vote.from));
    vote.amount = tx.amount;
    vote.favorite = tx.userdata[0];
    yield(collect);
}
```
The contract's object file implements a map function and lays out memory that is allocated per transaction. It then yields itself to a collect call that is schedule to run sometime later.
```
__section("collect.map.len")
const uint32_t votelen;
__section("collect.map.data")
const struct Vote vote[votelen];
__section("collect.data")
Vote totals[votelen];
__section("collect.data.used")
uint32_t used;
__section("collect")
void collect()
{
   used = sizeof(struct Vote) * votelen;
   memmove(totals, vote, used);
   yield(reduce)
}
Then we simply reduce multiple collect objects into 1 data structure. Reduce additionally sees its own previous output from multiple calls to reduce.
__section("reduce.collect.len")
const uint32_t len;
__section("reduce.collect.data")
const struct Vote *vote[len];
__section("reduce.collect.data.sizes")
const uint32_t sizes[len];
//partially reduced contracts are also available to reduce
//so it can fold over it's own output
__section("reduce.reduce.len")
const uint32_t rlen;
__section("reduce.reduce.data")
Vote *rtotals[rlen];
__section("reduce.reduce.data.sizes")
const uint32_t rsizes[rlen];
//total memory available for reduce `data`
//this is the sum of all the reduce and collect allocations
__section("reduce.data.total")
const uint32_t total;
__section("reduce.data")
Vote totals[total/sizeof(struct Vote)];
__section("reduce.data.used")
uint32_t used;
__section("reduce")
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
__section("finalize.reduce.data.used")
const uint32_t used;
__section("finalize.reduce.data")
Vote totals[used/sizeof(struct Vote)];
__section("finalize.data")
uint64_t votes[256];
__section("finalize")
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
