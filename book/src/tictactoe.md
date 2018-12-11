# Example program: Tic-Tac-Toe

This project demonstrates how to use the [Solana Javascript API](https://github.com/solana-labs/solana-web3.js)
to build, deploy, and interact with programs on the Solana blockchain, implementing an interactive tic-tac-toe game between two users.
To see the final product, go to https://solana-example-tictactoe.herokuapp.com/ and wait for another player to join.
(Simply direct a second browser window to the web app to play against yourself.)

The project comprises:

* The on-chain Tic-Tac-Toe program, a BPF program written in C: `program-bpf/`
* Easy program build and deployment using the `@solana/web3.js` library
* Command-line and web front-end: `src/`

## Getting Started

First fetch the latest release of the example code:

```sh
$ git clone https://github.com/solana-labs/example-tictactoe.git
$ cd example-tictactoe
$ TAG=$(git describe --tags $(git rev-list --tags --max-count=1))
$ git checkout $TAG
```

Then fetch the npm dependencies, including `@solana/web3.js`, by running:

```sh
$ npm install
```

### Select a Network
The example connects to a local Solana network by default.

To start a local Solana network run:
```bash
$ npx solana-localnet update
$ npm run localnet:up
```

Solana network logs are available with:
```bash
$ npx solana-localnet logs -f
```

For more details on working with a local network, see the [full instructions](https://github.com/solana-labs/solana-web3.js#local-network).

Alternatively to connect to the public testnet, `export LIVE=1` in your environment before running a front-end.

### Build the BPF C program
```sh
$ V=1 make -C program-bpf
```
or
```sh
$ npm run build:bpf
```

Compiled files can be found in `dist/program`. Compiler settings are configured in the [Solana SDK](https://github.com/solana-labs/solana/tree/master/sdk/bpf/bpf.mk)

### Run the Command-Line Front End
After building the program,

```sh
$ npm run start
```

This script uses the Solana Javascript API `BpfLoader` to deploy your Tic-Tac-Toe program to the blockchain.
Once the deploy transaction is confirmed on the chain, the script calls the program to instantiate a new dashboard
to track your open and completed games (`findDashboard`), and starts a new game (`dashboard.startGame`), waiting for an opponent.

To play the game, open a second terminal and again run the `npm run start` script.

To see the program or game state on the blockchain, send a `getAccountInfo` [JSON-RPC request](https://solana-labs.github.io/solana/jsonrpc-api.html#getaccountinfo) to the cluster, using the id printed by the script, eg.:
* `Dashboard programId: HFA4x4oZKWeGcRVbUYaCHM59i5AFfP3nCfc4NkrBvVtP`
* `Dashboard: HmAEDrGpsRK2PkR51E9mQrKQG7Qa3iyv4SvZND9uEkdR`
* `Advertising our game (Gx1kjBieYgaPgDhaovzvvZapUTg5Mz6nhXTLWSQJpNMv)`

### Run the WebApp Front End
After building the program,

```sh
$ npm run dev
```

This script deploys the program to the blockchain, and also boots up a local webserver
for game play.

To instantiate a dashboard and game, open your browser to http://localhost:8080/.

## Customizing the Program
To customize Tic-Tac-Toe, make changes to the program in `program-bpf/src` and rebuild.
Now when you run `npm run start`, you should see your changes.

To deploy a program with a different name, edit `src/server/config.js`.

