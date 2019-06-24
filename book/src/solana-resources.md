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

## Scripts

The purpose of scripts is to execute logic between process instruction calls.
For example, assignment of tokens to the winner based on an output of a game can
be defined as a script.

### `Loader::CreateScript`

`Loader::CreateScript` is similar to `Loader::CreateProcess` as it binds a
context to a program. The difference between scripts and processes is that
script execution yields to external process instructions.

### Script Execution

Transactions that include instructions that invoke scripts encode all the
instructions that the script will make during its execution in the message
instruction vector.  The additional instructions appear after the script, in the
exact same order and the same parameters as the script would make during
execution.

During the script execution, calls to instructions yield, and the next
instruction to be processed in the instruction vector is invoked.  The
instruction that is called must match the one that the script is expecting to
call through its execution path.  If the instruction doesn't match, the program
fails.

### Script Execution Example

```
//Swap some tokens around
//based on libra move examples

public main(payee: address, amount: u64 exchage_rate: f64) {
  let happy = 0x0.HappyCoin.withdraw_from_sender(copy(amount));
  0x0.HappyCoin.deposit(copy(payee), move(happy));
  //logic is interspersed with resource operations
  let amount = (amount * exchange_rate).floor() as u64;
  let sad = 0x0.SadCoin.withdraw(copy(payee), copy(amount));
  0x0.SadCoin.deposit_to_sender(move(sad));
}

```

The above script would contain an instruction vector that looks like

```
Message {
    instructions: vec![
        //instruction to the entry point of the above script
        script,
        //instruction that points to HappyCoin::withdraw
        happy_coin_withdraw,
        //instruction that points to HappyCoin::deposit
        happy_coin_deposit,
        //instruction that points to SadCoin::deposit_to_sender
        sad_coin_deposit_to_sender,
    ]
}
```

When the script reaches the following line:

```
let happy = 0x0.HappyCoin.withdraw_from_sender(copy(amount));
```

The script yields, and the next instruction in the instruction vector is
scheduled to execute.  Before it is executed, Pipeline checks that the call is
to `HappyCoin::withdraw_from_sender`, that the address is the source is the
sender the amount matches amount by verifying the instruction arguments declared
in the vector.

Effectively, calls to external processes are encoded as instructions, and the
runtime can validate that when the script yields the accounts and data
in the instruction match what is encoded in the message instruction vector.

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
