# Smart Contracts Engine
 
The goal of this RFC is to define a set of constraints for APIs and runtime such that we can execute our smart contracts safely on massively parallel hardware such as a GPU.  Our runtime is built around an OS *syscall* primitive.  The difference in blockchain is that now the OS does a cryptographic check of memory region ownership before accessing the memory in the Solana kernel.

## Version

version 0.2 

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

In Figure 1 an untrusted client, creates a program in the front-end language of her choice, (like C/C++/Rust/Lua), and compiles it with LLVM to a position independent shared object ELF, targeting BPF bytecode. Solana will safely load and execute the ELF.

## Runtime

The goal with the runtime is to have a general purpose execution environment that is highly parallelizeable and doesn't require dynamic resource management. The goal is to execute as many contracts as possible in parallel, and have them pass or fail without a destructive state change.


### State

State is addressed by an account which is at the moment simply the Pubkey.  Our goal is to eliminate memory allocation from within the smart contract itself.  Thus the client of the contract provides all the state that is necessary for the contract to execute in the transaction itself.  The runtime interacts with the contract through a state transition function, which takes a mapping of [(Pubkey,State)] and returns [(Pubkey, State')].  The State is an opeque type to the runtime, a `Vec<u8>`, the contents of which the contract has full control over.

### Call Structure
```
/// Call definition
/// Signed portion
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct CallData {
    /// Each Pubkey in this vector is mapped to a corresponding `Page` that is loaded for contract execution
    /// In a simple pay transaction `key[0]` is the token owner's key and `key[1]` is the recipient's key.
    pub keys: Vec<Pubkey>,

    /// The Pubkeys that are required to have a proof.  The proofs are a `Vec<Signature> which encoded along side this data structure
    /// Each Signature signs the `required_proofs` vector as well as the `keys` vectors.  The transaction is valid if and only if all
    /// the required signatures are present and the public key vector is unchanged between signatures.
    pub required_proofs: Vec<u8>,

    /// PoH data
    /// last PoH hash observed by the sender
    pub last_id: Hash,

    /// Program
    /// The address of the program we want to call.  ContractId is just a Pubkey that is the address of the loaded code that will execute this Call.
    pub contract_id: ContractId,
    /// OS scheduling fee
    pub fee: i64,
    /// struct version to prevent duplicate spends
    /// Calls with a version <= Page.version are rejected
    pub version: u64,
    /// method to call in the contract
    pub method: u8,
    /// usedata in bytes
    pub userdata: Vec<u8>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct Call {
    /// Signatures and Keys
    /// (signature, key index)
    /// This vector contains a tuple of signatures, and the key index the signature is for
    /// proofs[0] is always key[0]
    pub proofs: Vec<Signature>,
    pub data: CallData,
}
```

At it's core, this is just a set of Pubkeys and Signatures with a bit of metadata.  The contract Pubkey routes this transaction into that contracts entry point.  `version` is used for dropping retransmitted requests.

Contracts should be able to read any state that is part of runtime, but only write to state that the contract allocated.

### Execution

Calls batched and processed in a pipeline

```
+-----------+    +-------------+    +--------------+    +--------------------+    
| sigverify |--->| lock memory |--->| validate fee |--->| allocate new pages |--->
+-----------+    +-------------+    +--------------+    +--------------------+    
                                
    +------------+    +---------+    +--------------+    +-=------------+   
--->| load pages |--->| execute |--->|unlock memory |--->| commit pages |   
    +------------+    +---------+    +--------------+    +--------------+   

```

At the `execute` stage, the loaded pages have no data dependencies, so all the contracts can be executed in parallel. 
## Memory Management
```
pub struct Page {
    /// key that indexes this page
    /// prove ownership of this key to spend from this Page
    owner: Pubkey,
    /// contract that owns this page
    /// contract can write to the data that is in `memory` vector
    contract: Pubkey,
    /// balance that belongs to owner
    balance: u64,
    /// version of the structure, public for testing
    version: u64,
    /// hash of the page data
    memhash: Hash,
    /// The following could be in a separate structure
    memory: Vec<u8>,
}
```

The guarantee that runtime enforces:
    1. The contract code is the only code that will modify the contents of `memory`
    2. Total balances on all the pages is equal before and after exectuion of a call
    3. Balances of each of the pages not owned by the contract must be equal to or greater after the call than before the call.

## Entry Point
Exectuion of the contract involves maping the contract's public key to an entry point which takes a pointer to the transaction, and an array of loaded pages.
```
// Find the method
match (tx.contract, tx.method) {
    // system interface
    // everyone has the same reallocate
    (_, 0) => system_0_realloc(&tx, &mut call_pages),
    (_, 1) => system_1_assign(&tx, &mut call_pages),
    // contract methods
    (DEFAULT_CONTRACT, 128) => default_contract_128_move_funds(&tx, &mut call_pages),
    (contract, method) => //... 
```

The first 127 methods are reserved for the system interface, which implements allocation and assignment of memory.  The rest, including the contract for moving funds are implemented by the contract itself.

## System Interface
```
/// SYSTEM interface, same for very contract, methods 0 to 127
/// method 0
/// reallocate
/// spend the funds from the call to the first recipient's
pub fn system_0_realloc(call: &Call, pages: &mut Vec<Page>) {
    if call.contract == DEFAULT_CONTRACT {
        let size: u64 = deserialize(&call.userdata).unwrap();
        pages[0].memory.resize(size as usize, 0u8);
    }
}
/// method 1
/// assign
/// assign the page to a contract
pub fn system_1_assign(call: &Call, pages: &mut Vec<Page>) {
    let contract = deserialize(&call.userdata).unwrap();
    if call.contract == DEFAULT_CONTRACT {
        pages[0].contract = contract;
        //zero out the memory in pages[0].memory
        //Contracts need to own the state of that data otherwise a use could fabricate the state and
        //manipulate the contract
        pages[0].memory.clear();
    }
} 
```
The first method resizes the memory that is assosciated with the callers page.  The second system call assignes the page to the contract.  Both methods check if the current contract is 0, otherwise the method does nothing and the caller spent their fees.

This ensures that when memory is assigned to the contract the initial state of all the bytes is 0, and the contract itself is the only thing that can modify that state.

## Simplest contract
```
/// DEFAULT_CONTRACT interface
/// All contracts start with 128
/// method 128
/// move_funds
/// spend the funds from the call to the first recipient's
pub fn default_contract_128_move_funds(call: &Call, pages: &mut Vec<Page>) {
    let amount: u64 = deserialize(&call.userdata).unwrap();
    if pages[0].balance >= amount  {
        pages[0].balance -= amount;
        pages[1].balance += amount;
    }
}
``` 

This simply moves the amount from page[0], which is the callers page, to page[1], which is the recipient's page.

## Notes

1. There is no dynamic memory allocation.
2. Persistent Memory is allocated to a Key with ownership
3. Contracts can `call` to update key owned state
4. `call` is just a *syscall* that does a cryptographic check of memory ownership
5. Kernel guarantees that when memory is assigned to the contract its state is 0
6. Kernel guarantees that contract is the only thing that can modify memory that its assigned to
7. Kernel guarantees that the contract can only spend tokens that are in pages that are assigned to it
8. Kernel guarantees the balances belonging to pages are balanced before and after the call
