# Long-term storage

## Background

Accounts act as a form of persistent storage, allowing state to persist across
multiple transactions. To store data in accounts, however, is relatively
expensive compared to storing transactions on the ledger. Account storage
requires memory on *every* validator whereas transaction storage requires only
disk space on a handful of replicators. If a client does not plan to use
account data for a long time, it ought to have some means of swapping that
memory out to disk. This RFC proposes a way to store account data on the ledger
and reload it when needed.

## Moving data from account to ledger

First, the client can use the JSON RPC API to request all account data. That can
be done programically, but here's how it looks from the command-line:

```bash
# Request
$ curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0", "id":1, "method":"getAccountInfo", "params":["2gVkYWexTHR5Hb2aLeQN3tnngvWzisFKXDUPrgMHpdST"]}' http://localhost:8899

{"jsonrpc":"2.0","result":{"executable":false,"loader":[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],"owner":[1,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],"tokens":100,"userdata":[3,0,0,0,0,0,0,0,1,0,0,0,0,0,1,0,0,0,0,0,0,0,20,0,0,0,0,0,0,0,50,48,53,48,45,48,49,45,48,49,84,48,48,58,48,48,58,48,48,90,252,10,7,28,246,140,88,177,98,82,10,227,89,81,18,30,194,101,199,16,11,73,133,20,246,62,114,39,20,113,189,32,50,0,0,0,0,0,0,0,247,15,36,102,167,83,225,42,133,127,82,34,36,224,207,130,109,230,224,188,163,33,213,13,5,117,211,251,65,159,197,51,0,0,0,0,0,0]},"id":1}
```

Now that you have the data, reclaim any tokens stored in the account:

```rust
Transaction {
   keys: vec![
       2gVkYWexTHR5Hb2aLeQN3tnngvWzisFKXDUPrgMHpdST,
       my_account,
   ],
   instructions: vec![
       SystemInstruction::Move { tokens: 99 },
   ],
   fee: 1,
   ...
}
```

Next, send a transaction with only enough tokens to pay the transaction fee.

```rust
Transaction {
   keys: vec![
       my_account,
       2gVkYWexTHR5Hb2aLeQN3tnngvWzisFKXDUPrgMHpdST,
   ],
   instructions: vec![
       LoaderInstruction::Write { offset: 0, bytes: [3,0,0,0,0,0,0,0,1,0,0,0,0,0,1,0,0,0,0,0,0,0,20,0,0,0,0,0,0,0,50,48,53,48,45,48,49,45,48,49,84,48,48,58,48,48,58,48,48,90,252,10,7,28,246,140,88,177,98,82,10,227,89,81,18,30,194,101,199,16,11,73,133,20,246,62,114,39,20,113,189,32,50,0,0,0,0,0,0,0,247,15,36,102,167,83,225,42,133,127,82,34,36,224,207,130,109,230,224,188,163,33,213,13,5,117,211,251,65,159,197,51,0,0,0,0,0,0]},
       LoaderIntruction::Finalize,
   ],
   fee: 1,
   ...
}
```

Last, the client should store the transaction signature for when it wants to reload
that data. We'll reference that signature below with the bash variable `$MY_SIGNATURE`.


## Moving data from ledger to account

Use a new JSON RPC API to query for transaction data by its signature:

```bash
# Request
$ curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0", "id":1, "method":"getTransaction", "params":[$MY_SIGNATURE]}' http://localhost:8899
Transaction {
   keys: vec![
       my_account,
       2gVkYWexTHR5Hb2aLeQN3tnngvWzisFKXDUPrgMHpdST,
   ],
   instructions: vec![
       LoaderInstruction::Write { offset: 0, bytes: [3,0,0,0,0,0,0,0,1,0,0,0,0,0,1,0,0,0,0,0,0,0,20,0,0,0,0,0,0,0,50,48,53,48,45,48,49,45,48,49,84,48,48,58,48,48,58,48,48,90,252,10,7,28,246,140,88,177,98,82,10,227,89,81,18,30,194,101,199,16,11,73,133,20,246,62,114,39,20,113,189,32,50,0,0,0,0,0,0,0,247,15,36,102,167,83,225,42,133,127,82,34,36,224,207,130,109,230,224,188,163,33,213,13,5,117,211,251,65,159,197,51,0,0,0,0,0,0]},
       LoaderIntruction::Finalize,
   ],
   fee: 1,
   ...
}
```

Last, restore the account by sending that same transaction data with the remaining tokens:

```rust
Transaction {
   keys: vec![
       my_account,
       2gVkYWexTHR5Hb2aLeQN3tnngvWzisFKXDUPrgMHpdST,
   ],
   instructions: vec![
       SystemInstruction::Move { tokens: 98 },
       LoaderInstruction::Write { offset: 0, bytes: [3,0,0,0,0,0,0,0,1,0,0,0,0,0,1,0,0,0,0,0,0,0,20,0,0,0,0,0,0,0,50,48,53,48,45,48,49,45,48,49,84,48,48,58,48,48,58,48,48,90,252,10,7,28,246,140,88,177,98,82,10,227,89,81,18,30,194,101,199,16,11,73,133,20,246,62,114,39,20,113,189,32,50,0,0,0,0,0,0,0,247,15,36,102,167,83,225,42,133,127,82,34,36,224,207,130,109,230,224,188,163,33,213,13,5,117,211,251,65,159,197,51,0,0,0,0,0,0]},
       LoaderIntruction::Finalize,
       SystemInstruction::Assign { program_id: [1,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0] },
   ],
   fee: 1,
   ...
}
```

After restoring the account, it should look identical to the original, minus the
two transaction fees.

