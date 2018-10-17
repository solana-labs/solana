# Smart Contracts Engine
 
The goal of this RFC is to define a set of constraints for APIs and smart contracts runtime such that we can execute our smart contracts safely on massively parallel hardware such as a GPU.

## Version

Version 0.3 

## Definitions

* Transaction - an atomic operation with multiple instructions.  All Instruction must complete successfully for the transaction to be comitted.
* Instruction - a call to a program that modifies Account token balances and Account specific userdata state.  A single transaction may have multiple Instructions with different Accounts and Programs.
* Program - Programs are code that modifies Account token balances and Account specific userdata state.
* Account - A single instance of state.  Accounts are looked up by account Pubkeys and are associated with a Program's Pubkey.

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

The goal with the runtime is to have a general purpose execution environment that is highly parallelizeable and doesn't require dynamic resource management. The goal is to execute as many programs as possible in parallel, and have them pass or fail without a destructive state change.


### State

State is addressed by an Account which is at the moment simply the Pubkey.  Our goal is to eliminate memory allocation from within the program itself.  Thus the client of the program provides all the state that is necessary for the program to execute in the transaction itself.  The runtime interacts with the program through an entry point with a well defined interface.  The userdata stored in an Account is an opaque type to the runtime, a `Vec<u8>`, the contents of which the program code has full control over.

### Transaction structure
```
/// An atomic transaction
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct Transaction {
    /// A digital signature of `account_keys`, `program_ids`, `last_id`, `fee` and `instructions`, signed by `Pubkey`.
    pub signature: Signature,

    /// The `Pubkeys` that are executing this transaction userdata.  The meaning of each key is
    /// program-specific.
    /// * account_keys[0] - Typically this is the `caller` public key.  `signature` is verified with account_keys[0].
    /// In the future which key pays the fee and which keys have signatures would be configurable.
    /// * account_keys[1] - Typically this is the program context or the recipient of the tokens
    pub account_keys: Vec<Pubkey>,

    /// The ID of a recent ledger entry.
    pub last_id: Hash,

    /// The number of tokens paid for processing and storage of this transaction.
    pub fee: i64,

    /// Keys identifying programs in the instructions vector.
    pub program_ids: Vec<Pubkey>,
    /// Programs that will be executed in sequence and commited in one atomic transaction if all
    /// succeed.
    pub instructions: Vec<Instruction>,
}
```

The Transaction structure specifies a list of Pubkey's and signatures for those keys and a sequentail list of instructions that will operate over the state's assosciated with the `account_keys`.  For the transaction to be committed all the instructions must execute successfully, if any abort the whole transaction fails to commit.

### Account structure
Accounts maintain token state as well as program specific memory.
```
/// An Account with userdata that is stored on chain
pub struct Account {
    /// tokens in the account
    pub tokens: i64,
    /// user data
    /// A transaction can write to its userdata
    pub userdata: Vec<u8>,
    /// program id this Account belongs to
    pub program_id: Pubkey,
}
```

# Transaction Engine

At it's core, the engine looks up all the Pubkeys maps them to accounts and routs them to the `program_id` entry point.

## Execution

Transactions are batched and processed in a pipeline

```
+-----------+    +-------------+    +--------------+    +--------------------+    
| sigverify |--->| lock memory |--->| validate fee |--->| allocate accounts  |--->
+-----------+    +-------------+    +--------------+    +--------------------+    
                                
    +------------+    +---------+    +-=------------+   +--------------+
--->| load data  |--->| execute |--->| commit data  |-->|unlock memory |
    +------------+    +---------+    +--------------+   +--------------+

```

At the `execute` stage, the loaded pages have no data dependencies, so all the programs can be executed in parallel. 

The runtime enforces the following rules:

1. The `program_id` code is the only code that will modify the contents of `Account::userdata` of Account's that have been assigned to it.  This means that upon assignment userdata vector is guarnteed to be `0`.
2. Total balances on all the accounts is equal before and after execution of a Transaction.
3. Balances of each of the accounts not assigned to `program_id` must be equal to or greater after the Transaction than before the transaction.
4. All Instructions in the Transaction executed without a failure.

## Entry Point
Execution of the program involves mapping the Program's public key to an entry point which takes a pointer to the transaction, and an array of loaded pages.

```
pub fn process_transaction(
    tx: &Transaction,
    pix: usize,
    accounts: &mut [&mut Account],
) -> Result<()>;
```


## System Interface
```
pub enum SystemProgram {
    /// Create a new account
    /// * Transaction::keys[0] - source
    /// * Transaction::keys[1] - new account key
    /// * tokens - number of tokens to transfer to the new account
    /// * space - memory to allocate if greater then zero
    /// * program_id - the program id of the new account
    CreateAccount {
        tokens: i64,
        space: u64,
        program_id: Pubkey,
    },
    /// Assign account to a program
    /// * Transaction::keys[0] - account to assign
    Assign { program_id: Pubkey },
    /// Move tokens
    /// * Transaction::keys[0] - source
    /// * Transaction::keys[1] - destination
    Move { tokens: i64 },
}
```
The interface is best described by the `Instruction::userdata` that the user encodes. 
* `CreateAccount` - This allows the user to create and assign an Account to a Program.
* `Assign` - allows the user to assign an existing account to a `Program`. 
* `Move`  - moves tokens between `Account`s that are assosciated with `SystemProgram`.  This cannot be used to move tokens of other `Account`s.  Programs need to implement their own version of Move.

## Notes

1. There is no dynamic memory allocation.  Client's need to call the `SystemProgram` to create memory before passing it to another program.  This Instruction can be composed into a single Transaction with the call to the program itself.
2. Runtime guarantees that when memory is assigned to the program it is zero initialized.
3. Runtime guarantees that program's code is the only thing that can modify memory that its assigned to
4. Runtime guarantees that the program can only spend tokens that are in Accounts that are assigned to it
5. Runtime guarantees the balances belonging to Accounts are balanced before and after the transaction
6. Runtime guarantees that multiple instructions all executed successfully when a transaction is committed.
