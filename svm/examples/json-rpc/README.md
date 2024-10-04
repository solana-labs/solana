This is an example application using SVM to implement a tiny subset of
Solana RPC protocol for the purpose of simulating transaction
execution without having to use the entire Solana Runtime.

The exmample consists of two host applications
- json-rpc-server -- the RPC server that accepts incoming RPC requests
  and performs transaction simulation sending back the results,
- json-rpc-client -- the RPC client program that sends transactions to
  json-rpc-server for simulation,

and

- json-rpc-program is the source code of on-chain program that is
  executed in a transaction sent by json-rpc-client.

To run the example, compile the json-rpc-program with `cargo
build-sbf` command. Using solana-test-validator create a ledger, or
use an existing one, and deploy the compiled program to store it in
the ledger. Using agave-ledger-tool dump ledger accounts to a file,
e.g. `accounts.out`. Now start the json-rpc-server, e.g.
```
cargo run --manifest-path json-rpc-server/Cargo.toml -- -l test-ledger -a accounts.json
```

Finally, run the client program.
```
cargo run --manifest-path json-rpc-client/Cargo.toml -- -C config.yml -k json-rpc-program/target/deploy/helloworld-keypair.json -u localhost
```

The client will communicate with the server and print the responses it
recieves from the server.
