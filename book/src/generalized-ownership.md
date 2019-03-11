# Generalized Ownership

The current implementation of user-defined tokens cannot be exchanged by
other user-defined programs.

This design describes generalized ownership of countable items on the Solana
blockchain. The goal of this design is to propose a single general-purpose
program whose state can be used across user-defined programs to implement
ownership and transfer.

Many programs in Solana should be able to use a common instruction similar to
`SystemInstruction::Transfer`.  Furthermore, the amounts, destinations and sources
should be based on outputs of programs as well as specified in transactions,
while maintaining the same security guarantees of transfer of native tokens.

## Goal for this design

The goal of this design is to make the following possible:

In a single program, it should be possible to change and update ownership of
arbitrary units of tokens implemented by other programs.

Alice requests X amount of token implemented by program Token1, in
exchange for Y amount of token implemented by program Token2.

Bob offers X + 1 amount of token implemented by program Token1, in exchange for
Y - 1 amount of token implemented by Token2.

Dan pairs Bob and Alice, causing the resulting ownership changes:

* Alice owns X of Token1,
* Bob owns Y - 1 of Token2,
* Dan owns 1 of Token1 and 1 of Token2,

Another example:

Alice offers X amount of Token1 to the winner of TicTacToe.

Bob offers Y amount of Token2 to the winner of the same game of TicTacToe.

The game charges a commission, the ceiling of a percentage of the tokens.  The
winner takes ownership of both tokens minus the commission.  The commission is
transferred to the address designated by the game.

## Current System and Token programs

The System program defines ownership of lamports to be identified by the public key
that matches the account's pubkey. The`SystemInstruction::Transfer` instruction checks that
the identity of the spending account has a valid signature before completing the
transfer.

The Token program defines ownership of the token to be identified by the public key
that matches the account's pubkey, and `Token::Transfer` instruction checks
that the identity of the spending account has a valid signature before
completing the transfer.

Both of these programs implement almost the same mechanism.
 
## General idea

For items to have externally countable ownership, they must rely on the external
program for managing the transfer of units.  These items must have a valid state
where the number of units owned is 0.  The **zero** count of items is
the destination for transferring any count of items on the chain.

Execution of these state transitions must allow for a reentrant and partial
transaction to make progress.

## Generalized owner identity

Using the account's pubkey identity as the asset owner identity means that all
the asset transfer APIs must be re-implemented by the program code.

The proposed solution is to use a pointer to an owner identity.

```rust,ignore
// This state is owned by the Owner program
enum OwnerState {
    // the owner identity must provide a valid signature for this pubkey
    Unique {
        // The actual owner key.
        owner: Pubkey, 
        // The item program instance.
        item: Pubkey
    },
    //...
}
```

* OwnerState::Unique.owner - The actual OwnerState’s account’s pubkey is not used to
distinguish ownership.  Only the `OwnerState::Unique.owner` pubkey identifies the
ownership identity.

```rust,ignore
// This state is owned by the MyUniqueTokenState
struct MyUniqueTokenState {
    // This points to an OwnerState structure that identifies the actual owner
    owner: Pubkey,
}
```

* MyUniqueStokenState.owner - address of the OwnerState that identifies the
current owner.

### OwnerState::Transfer

* account[0] - RW - The OwnerState.
* account[1] - R - Current owner Pubkey, needs a signature. Doesn’t need an
allocated Account.
* data - New owner Pubkey. Doesn’t need an allocated Account.

### MyTokenState::Init

* account[0] - RW - MyUniqueTokenState. MyUniqueTokenState initializes the
owner once with `account[1]`.  This program must never modify that field in any
future state transitions.
* account[1] - R - The OwnerState.  OwnerState::Unique.item must be equal to
account[0].

### OwnerState::Countable

The item that is owned by this OwnerState must have a valid count `0.`
representation that the user can create.  Units are then transferred from one
`OwnerState::Countable` into another, and the total sum is preserved between
transfers.

```rust,ignore
// This state is owned by the Owner program
enum OwnerState {
    //...
    Countable {
        // The owner identity must provide a valid signature for this pubkey
        owner: Pubkey,
        // The number of units this OwnerState represents
        units: u64,
        // The item program instance.
        item: Pubkey,
    },
    //...
}
```

When `OwnerState::Countable::units == 1`, this object is exactly the same as
`OwnerState::Unique`.  The reason for a separate `OwnerState::Unique` is to
programmatically ensure that only a single instance of this item exists.

```rust,ignore
// This state is owned by the MyTokenState
struct MyTokenState {
    // This points to an OwnerState structure that identifies the actual owner
    owner: Pubkey,
}

```

The MyToken program must expect many `OwnerStates` to point to the same instance
of `MyTokenState`

## Programmatic Transfer of Ownership

Currently, programs have no way to trigger a transfer of ownership. A game of
TicTacToe has no way to assign a prize to the winner.

```rust,ignore
// This state is owned by the Owner program
enum OwnerState {
    //...
    Escrow {
        // The number of units this OwnerState represents, or a Unique item.
        units: Option<u64>,
        // Number of already claimed outputs
        claimed: u64,
        // The item program instance.
        item_program: Pubkey,
        // The state stored inside this pubkey will be the new owner
        escrow: Pubkey,
    },
}
type EscrowClaims =  (u64, Vec<(Pubkey,u64)>);
```

* `OwnerState::Escrow::escrow` - The account data must deserialize
to `EscrowClaims`. 

When the account contains a vector for all the `OwnerState::Escrow.units` the
`OwnerState::Escrow` can be converted into an `Owner::Countable` types.  The
first element `u64` is unused, and is there for the program that owns the
account to use as a header indicating the type of account data that is stored.


### Owner::EscrowClaimUnique instruction

* account[0] - RW - `OwnerState::Escrow`.
* account[1] - R - The Account at the address of `OwnerState::Escrow.escrow`.

If `account[1]` data deserializes into, a `EscrowClaims` the count must be
1, and the `account[0]` state is transformed into the `OwnerState::Unique` object.

### Owner::EscrowClaimCountable instruction

* account[0] - RW - `OwnerState::Escrow`.
* account[1] - R - The Account at the address of `OwnerState::Escrow.escrow`.
* account[2] - RW - `OwnerState::Countable`
* account[3] - R - `Account` item account.  This `Account.owner` must match
 `OwnerState::Escrow.item_program`.                                         

If `account[1]` data deserializes into an `EscrowClaims`, the count in
`account[2]` is updated with the claimed output, and the `Escrow.claimed` is
incremented.  The claimed units are added to `account[2]`.

### Example: 

An offer to trade:

```rust,ignore
Trade {
    //address to the OwnerState::Escrow
    offer: OwnerState::Escrow {    
        units: Some(100),
        item: Token1,
        escrow: Match, //address 0
    }
    ask: Ask {    
        units: 100,
        item: Token2,
        identity: Pubkey
    } 
}
```

Another offer to trade:

```rust,ignore
Trade {
    //address to the OwnerState::Escrow
    offer: OwnerState::Escrow {    
        units: 101,
        item: Token2,
        escrow: Match, //address 1
    }
    ask: Ask {    
        units: 99,
        item: Token1,
        dentity: Pubkey
    } 
}
``` 

Exchange::Match instruction

account[0] - R -  Trade[0]
account[1] - RW - Match address 0
account[2] - R -  Trade[1]
account[3] - RW - Match address 1

Match addresses now contain the new owner keys.  An `Owner::EscrowClaim`
instruction can them convert each Escrow into the expected outputs.  Once the
match address is set, the trade cannot be executed again.

## Challenges

This proposal relies heavily on passing many different account references in a
single transaction.  It is therefore unlikely to fit within a 512-byte restriction
per transaction.

The way to tackle these problems is to force the transactions to be reentrant
and evaluated partially.

Two transactions: 

* Exchange::Match instruction for Match address 1

account[0] - R -  Trade[0]
account[2] - R -  Trade[1]
account[3] - RW - Match address 1

* Exchange::Match instruction for Match address 2

account[0] - R -  Trade[0]
account[2] - R -  Trade[1]
account[3] - RW - Match address 2

Match 1 and Match 2 must result in the same output regardless of order and how
many times each instruction is called.
