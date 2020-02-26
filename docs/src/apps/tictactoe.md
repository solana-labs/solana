# Example: Tic-Tac-Toe

[Click here to play Tic-Tac-Toe](https://solana-example-tictactoe.herokuapp.com/) on the Solana testnet. Open the link and wait for another player to join, or open the link in a second browser tab to play against yourself. You will see that every move a player makes stores a transaction on the ledger.

## Build and run Tic-Tac-Toe locally

First fetch the latest release of the example code:

```bash
$ git clone https://github.com/solana-labs/example-tictactoe.git
$ cd example-tictactoe
$ TAG=$(git describe --tags $(git rev-list --tags
--max-count=1))
$ git checkout $TAG
```

Next, follow the steps in the git repository's [README](https://github.com/solana-labs/example-tictactoe/blob/master/README.md).

## Getting lamports to users

You may have noticed you interacted with the Solana cluster without first needing to acquire lamports to pay transaction fees. Under the hood, the web app creates a new ephemeral identity and sends a request to an off-chain service for a signed transaction authorizing a user to start a new game. The service is called a _drone_. When the app sends the signed transaction to the Solana cluster, the drone's lamports are spent to pay the transaction fee and start the game. In a real world app, the drone might request the user watch an ad or pass a CAPTCHA before signing over its lamports.

