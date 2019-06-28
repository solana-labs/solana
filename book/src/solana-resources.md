# Resources

This architecture describes how the Pipeline runtime supports resources. 

Resources are inspired by the Move whitepaper
(https://developers.libra.org/docs/move-paper).  The goal of this architecture
is provide similar safety guarantees to native Solana programs as Move
interpreter provides to resources.  The resource implementation governs all the
state transitions for the resource data, regardless of what user the resource
belongs to, including interpreting the meaning of signatures.

## Processes

The purpose of a process is to allow for implementation reuse of well known
programs for different purposes.

Programs are loaded by the Loader as BPF bytecode and are identified by the
pubkey of the program.  Programs are pure code without a state.  Process is a
program with a context.  The context is always passed as a credit-only first
parameter to all program instructions for the process.

Resources are implemented as Processes.

### `Loader::CreateProcess`

The ‘Loader::CreateProcess’ instruction is used to create a specific instance of
a program with an associated read-only context. Once a process has been created,
it can be used as an account "owner".

Loaders create processes by pairing a program pubkey and a context.  The
pipeline will pass the context as the first parameter to all the program
instructions.  Initial implementation of `Loader::CreateProcess` only supports a
single read-only context that is fixed as the first parameter.

## Accounts Organization for Programs and Processes

Accounts are organized such that a single account can be used for storing
state of any process.  Process accounts themselves can store state.

### Account Map

Accounts are a map of an address to an account.

* `Accounts = Map<AccountAddress, Account>`

### Account Address

The `AccountAddress` is a hash of the account pubkey that users self-generate
and the owner pubkey.  With this change, a user only needs one pubkey, and it
exists for all owners.  The Account database stores the Account pubkey
and the Owner pubkey such that the index is recoverable.

* `AccountAddress = hash(Account pubkey, Owner pubkey)`

* `Accounts = Map<AccountAddress, Account>`

* `owner: Pubkey` - The process responsible for the state transitions of the
tokens and data in the ‘AccountAddress.’

### Account memory Allocation

* `System::Allocate`

This instruction is available to every program or process and appears as
instruction 0.

* `size: u64` - allocate the memory length in ‘size’ in bytes.  The memory is
zero-initialized.
