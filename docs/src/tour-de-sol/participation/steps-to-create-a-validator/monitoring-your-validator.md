# Validator catch-up

Once your validator is connected and voting, it should appear on the [Solana Network Explorer](http://explorer.solana.com/validators). \(Switch to the Tour de SOL network in the top right\)

But your validator may take some time to catch up with the cluster. Use the `catchup`  command to monitor this process:
```bash
solana catchup ~/validator-keypair.json
```

Until your validator has caught up, it will not be able to vote successfully and stake cannot be delegated to it.

Also if you find the network's slot advancing faster than yours, you will likely never catch up. This typically implies some kind of networking issue between your validator and the rest of the network.
