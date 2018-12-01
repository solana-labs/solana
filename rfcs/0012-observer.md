# Observer

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
        result: Pubkey,
        // address of TransferbleItem account
        observer: Pubkey,
    },
    // address of the winner
    Winner(Option<Pubkey>),
}
```

The Subject doesn't need to actually store the `observer` or `result` addresses.  But if the subject stores them and the clients can follow the structure it is easy for runtime programs to verify that the pointers actually point to the right place.  It is also possible to observe an offset directly into the state of the game.

### Observer

```
enum TransferableItem {
    Owned {
        owner_id: Pubkey
    },
    TransferOnOutput {
        //8, size of enum descriminator for data in TicTacToe::Winner
        output_msg_offset:  u64,

        //points to TicTacToe::Winner account address
        output_id:  Pubkey,
    },
}
```

TransferableItem is agnostic to the definition of the actual item whose ownership is transfered, or the output which decides the ownership of the item.

```
struct Prize {
    //points to a TransferableItem
    owner: Pubkey,
}
```

`Prize` just needs to honor the value of Item.owner in its code.

### Initialize Game
Inputs:
* account[0] - owner of item
* account[1] - game state
* account[2] - game result `TicTacToe::Winner`
* account[3] - `TransferableItem::Owned` instance

This would atomically call `TicTacToe::init` and `TransferableItem::init_transfer_on_output`, to simultaneously configure both.

#### `TicTacToe::init`
* accounts: [0,1,2] - initialize the game to point to the right result and observer.

#### `TransferableItem::init_transfer_on_output`
* accounts: [0,3,2] - Transmute `account[3]` from `TransferableItem::Owned` to `TransferableItem::TransferOnOutput`, and configure the output.

### `TransferableItem::claim`

* account[0] - `TransferableItem::TransferOnOutput` instance
* account[1] - `TicTacToe::Winner` instance

When this function executes, it checks that `account[1]` is the right `TicTacToe::Winner` and it is `Some(Pubkey)`. This function will transmute itelf to `TransferableItem::Owned` if the result actually deserializes to the right thing.
