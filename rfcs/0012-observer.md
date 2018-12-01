# IPC
 
The goal of this RFC is to define a set of conventions for securely observing changes between programs such that programs can be reused without modifications.

## Terminology

* subject - The program that generates the result.
* observer - The program that observes the result.

## Challenges

Programs are only executed by user driven transactions that call them via instructions.  Programs have no means of calling each other directly.  The observer cannot be tricked into receiving the result by presenting it with direct or indirect user manufactured state.

## Subject Design

Subject programs state is split up into a `computation` portion and a `output` portion.

```
enum ProgramState {
    Computation{
        output: Pubkey,
        observer: Pubkey,
    },   
    Output(Option<ProgramOutput>),   
}
```

* `ProgramState::Computation.output` - This value contains the `Account` address which the `ProgramState::Output` is stored.
* `ProgramState::Computation.observer` - This value contains the `Account` address which the observer is stored.
* `ProgramState::Output` - This value is stored in an `Account.userdata`, and the data indicates the output of the program.  When it is initialized the value is `ProgramState::Output(None)`, to indicate that no output has been produced yet.

The program runs until it produces an output, which it stores in the `ProgramState::Computation.output` account userdata as `ProgramState::Output(Some(ProgramOutput))`.  The program's code guarantees that output can only be written to `ProgramState::Computation.output`, and a different address could not be swapped.  At program initialization, 2 Accounts are required, one for `ProgramState::Computation` and the other one for `ProgramState::Output`.


## Observer Design

Observer is configured with the following data.

* Subject output enum value.  In the above case this would be `1`.
* Subject output account address.  This would be the same as `ProgramState::Computation.output`.

```
struct Observer {
    // enum descrimnator serialization size
    output_msg_offset: u64,
    output_id: Pubkey,
}
```


## Example

* Subject: TicTacToe
* Observer: TransferableItem 

### Subject

```
enum TicTacToe {
    Computation{
        // address of TicTacToe::Winner account
        winner: Pubkey,
        // address of TransferbleItem::TransferOnOutput account
        observer: Pubkey,
    },   
    // address of the winner
    Winner(Option<Pubkey>),   
}
```

### Observer

```
enum TransferableItem {
    Item { 
        owner_id: Option<Pubkey>
    },
    TransferOnOutput {
        //8, size of enum descriminator for data in TicTacToe::Winner
        output_msg_offset:  u64,

        //points to TicTacToe::Winner account address
        output_id:  Pubkey, 

        //TransferableItem::Item to set
        item: Pubkey,
    },
}
```

TransferableItem is agnostic to the definition of the actual item whose ownership is transfered.

```
struct Prize {
    //points to a TransferableItem::Item
    owner: Pubkey,
}
```

`Prize` just needs to honor the value of Item.owner in its code.

### TransferOnOutput

* account[0] - TransferableItem::TransferOnOutput instance
* account[1] - TransferOnOutput.item instance
* account[2] - TicTacToe::Winner instance

When this function executes, it checks that `Winner` deserializes into `Some(Pubkey)`.  If that succeeds it serializes `Some(Pubkey)` into the TransferOnOutput.item userdata.
