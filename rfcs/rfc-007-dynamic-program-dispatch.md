
This document highlights a set of changes to enable a framework for Solana
programs to be composed and dynamically dispatched.

### Account additions

```diff
diff --git a/common/src/account.rs b/common/src/account.rs
index 5abd642..89290ea 100644
--- a/common/src/account.rs
+++ b/common/src/account.rs
@@ -11,6 +11,12 @@ pub struct Account {
     pub userdata: Vec<u8>,
     /// contract id this contract belongs to
     pub program_id: Pubkey,
+
+    /// this account contains a program (and is strictly read-only)
+    pub executable: bool,
+
+    /// the loader for this program (Pubkey::default() for no loader)
+    pub loader_program_id: Pubkey,
 }

 impl Account {
@@ -19,6 +25,8 @@ impl Account {
             tokens,
             userdata: vec![0u8; space],
             program_id,
+            false,
+            Pubkey::default(),
         }
     }
 }
```


The executable flag may only be set by the `program_id` the account belongs to.
Once the executable flag is set:
1. The bank enforces the account userdata read-only even by `program_id`
2. The executable flag cannot be unset
3. The account may be used as input to `SystemProgram::Spawn` to forge a new `program_id`

The `loader_program_id` field is set by `SystemProgram::Spawn` and is used by
bank to locate the required loader during transaction dispatch.

Note: The `executable` and `loader_program_id` field would probably be better represented
by turning `userdata` into an enum such as: `Data(blob: Vec<u8>)`,
`FinalizedProgram(program: Vec<u8>)`, `Program(program: Vec<u8>, loader: Pubkey)`.

### Common Loader Interface

```rust
//! Instructions that must be implemented by all Loaders

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum LoaderInstruction {
    /// Write program data into an account
    ///
    /// * key[0] - the account to write into.
    ///
    /// The transaction must be signed by key[0]
    Write { offset: u32, prog: Vec<u8> },

    /// Finalize an account loaded with program data for execution.
    /// The exact preparation steps is loader specific but on success the loader must set the executable
    /// bit of the Account
    ///
    /// * key[0] - the account to prepare for execution
    ///
    /// The transaction must be signed by key[0]
    Finalize,
}
```

`LoaderInstruction` is only used for transactions where the account at
`Transaction::key[N]` is not executable.

The loader also includes the *program interpreter* within it.  On transactions
where the `Transaction::key[N]` account is executable and has a
`loader_program_id`, the loader instead invokes the program in
`Transaction::key[N]` passing along the transaction userdata and only
`Transaction::key[1..N-1]`.


### SystemProgram additions

```rust
    /// Transforms `executable_account_id` into a valid `program_id` for future transactions.
    /// This transaction must be signed by `executable_account_id`.
    ///
    /// executable_account_id: An account with its `executable` flag set and non-empty `loader_program_id()`
    ///
    /// On success:
    /// * The current `program_id` of `executable_account_id` will be saved in `loader_program_id`
    /// * The `program_id` of `executable_account_id` will be set to `executable_account_id`
    ///
    SystemProgram::Spawn{
        executable_account_id: Pubkey,
    }
```

### Native Loader

NativeLoader implements the `LoaderInstruction` interface as follows:

1. User writes the name of the native module (`solua`, `move_funds`, `noop`) into the account userdata using `LoaderInstruction::Write`
2. On `LoaderInstruction::Finalize`, NativeLoader:
    1. loads the corresponding native module into memory
    2. modifies the account userdata to contain a 0-based index into its internal vector of loaded shared objects
    3. sets the executable bit on the account

Note: certainly before mainnet, the NativeLoader's `LoaderInstruction` interface would be
disabled/removed.  Once this occurs only the native programs added during bank boot-up will be
available (described below).


### Solua Loader

SoluaLoader is a native module, loaded by NativeLoader, that implements the `LoaderInstruction` interface as follows:
1. User writes the contents of the desired Lua script into the account userdata using `LoaderInstruction::Write`
2. On `LoaderInstruction::Finalize`, SoluaLoader:
    1. parses the account userdata to confirm it's a valid Lua program
    2. sets the executable bit on the account


### Bpf Loader

BpfLoader is a native module, loaded by NativeLoader, that implements the `LoaderInstruction` interface as follows:
1. User writes the contents of the desired Bpf program into the account userdata using `LoaderInstruction::Write`
2. On `LoaderInstruction::Finalize`, BpfLoader:
    1. validates the Bpf byte code
    2. sets the executable bit on the account


### Bank Changes

bank.rs has only two hard-coded programs:
1. SystemProgram
2. NativeLoader

At bank boot-up, Rust functions of NativeLoader are invoked to load the
well-known, but natively implemented, program_ids, such as StorageProgram,
BpfLoader, *WasmLoader*, *EvmLoader*, ...


#### Bank program dispatch

When a transaction arrives with a program_id other than SystemProgram's or
NativeLoader's, it is dispatched as a generic program:

First the bank builds a list of program_id's chaining back to NativeLoader as follows:
```
- set current_program_id to the transacation's program_id
- set output_program_id_chain to []
- loop
   - abort if the account corresponding to current_program_id has no loader_program_id
   - abort if too many loop iterations (say ~5)
   - abort if current_program_id is already in output_program_id_chain
   - exit loop if current_program_id = NativeLoader's program_id
   - append current_program_id to output_program_id_chain
   - set current_program_id to the loader_program_id of the account
```

Then the bank appends output_program_id_chain to the end of the transaction's keys and
invokes `NativeLoader::process_transaction()`.


#### Example: Lua script program dispatch

When a transaction is sent to a lua script program_id:

1. transaction.program_id = lua_account (account with lua script as user data)
2. lua_account.loader_program_id = solua_program_id
3. solua_program_id = solua_account (account with an indexing into libsolua.so as user data)
4. solua_account.loader_program_id = nativeloader_program_id

The transaction would thus have two keys appended by the bank:
* `key[N+1]` = lua_account
* `key[N+2]` = solua_account

The bank then dispatches to NativeLoader.

NativeLoader removes `key[N+2]` as it dispatches to SoluaLoader

SoluaLoader loads the script from `key[N+1]`, and removes `key[N+1]` before interpreting it.

Lua script only sees `key[0..N]` as specified in the original transaction.
