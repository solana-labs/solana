# Solana Scripts

This architecture describes how the Pipeline runtime supports scripts. 

In Solana programs have absolute control over the state assigned to them.  The
goal of this architecture is provide a simple way for on-chain programs to
interact with other programs without violating the state ownership guarantees.

Contract composition has led to multiple multi-million dollar bugs in Ethereum

* https://www.parity.io/security-alert-2/

## Scripts

The purpose of scripts is to execute logic between instructions.

For example, assignment of tokens to the winner based on an output of a game can
be defined as a script.

### `Loader::CreateScript`

`Loader::CreateScript` designates that the loaded executable bytecode is a
script. The difference between scripts and programs is that script execution
yields to external process instructions.

### Script Execution

The general idea to script execution is that the client knows a head of time
what branches the script will take, and therefore can encode all the expected
instructions the script will generate into the transaction.

Transactions that include instructions that invoke scripts encode all the
instructions that the script will make during its execution in the message
instruction vector.  The additional instructions appear before the script, in the
exact same order and the same parameters as the script would make during
execution.

During the script execution, calls to instructions "yield", and the next
instruction to be processed in the instruction vector is invoked.  The
instruction that is called must match the one that the script is expecting to
call through its execution path.  If the instruction doesn't match, the program
fails.  During script execution, the runtime simply verifies that the
instruction invoked by the script is exactly the same as the one encoded by the
user in the message instruction vector.

### Script Execution Example

```
//Swap some tokens for lamports
pub fn process_instruction(
    program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    data: &[u8],
) -> Result<(), InstructionError> {
  let amounts: (u64, u64) = deserialize(data)?;
  let to_alice = Message::new(vec![system_instruction::transfer(keyed_accounts[0], keyed_accounts[1], amounts.0)];
  send_message(to_alice)?;
  let to_bob = Message::new(vec![token_instruction::transfer(keyed_accounts[2], keyed_accounts[3], amounts.1)];
  send_message(to_bob)?;
}

```

To successfully call the above script, the user would submit a transaction that
contains an instruction vector that looks like

```
Message {
    instructions: vec![
        //first instruction that is called by the script
        system_instruction_transfer,
        //second instruction that is called by the script
        token_instruction_transfer,
        //instruction to the entry point of the above script
        script_instruction,
    ]
}
```

When the script reaches the following line:

```
  send_message(to_alice)?;
```

The script "yields", and the next instruction in the instruction vector is
checked.  The runtime checks that the address is the source is the sender the
amount matches amount by verifying the instruction arguments declared in the
vector.

Effectively, calls to external processes are encoded as instructions, and the
runtime can validate that when the script yields the accounts and data
in the instruction match what is encoded in the message instruction vector.

### Signatures and Permissions

`Loader::CreateScript` may be authorize specific pubkeys that the script and
only the script can sign with.  To ensure that these keys cannot be signed by
the user, the addresses are derived from a sha256 of the script pubkey and a
the key sequence number.

* `sign_msg(msg: &mut Message, key_index: u64)`

Signs the message for the key `sha256.hash(program_id).hash(key_index)`.  Users
can generate these keys locally and encode them into the instruction vector.
During the script execution, the script will call `sign_msg` and attach the
appropriate signature.

