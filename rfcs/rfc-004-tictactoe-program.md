
Two players want to play tic-tac-toe with each other on Solana.

The tic-tac-toe program has already been provisioned on the network, and the
program author has advertised the following information to potential gamers:
* `tictactoe_publickey` - the program's public key
* `tictactoe_gamestate_size` - the number of bytes needed to maintain the game state

The game state is a well-documented data structure consisting of:
- Player 1's public key
- Player 2's public key
- Game status. An 8-bit value where:
  * 0 = game uninitialized
  * 1 = Player 1's turn
  * 2 = Player 2's turn
  * 3 = Player 1 won
  * 4 = Player 2 won
- Current board configuration.  A 3x3 character array containing the values '\0', 'X' or 'O'

### Game Setup

1. Two players want to start a game.  Player 2 sends Player 1 their public key,
`player2_publickey` off-chain (IM, email, etc)

2. Player 1 creates a new keypair to represent the game state, `(gamestate_publickey,
gamestate_privatekey)`.

3. Player 1 issues an allocate_memory transaction, assigning that memory page to the
tic-tac-toe program.  The `memory_fee` is used to *rent* the memory page for the
duration of the game and is subtracted from current account balance of Player
1:
```
allocate_memory(gamestate_publickey, tictactoe_publickey, tictactoe_gamestate_size, memory_fee)
```


4. Game state is then initialized by issuing a *new* call transaction to the
tic-tac-toe program.  This transaction is signed by `gamestate_privatekey`, known only
to Player 1.
```
call(tictactoe_publickey, gamestate_publickey, 'new', player1_publickey, player2_publickey)
```

5. Once the game is initialized, Player 1 shares `gamestate_publickey` with
Player 2 off-chain (IM, email, etc)

Note that it's likely each player prefer to generate a game-specific keypair
rather than sharing their primary public key (`player1_publickey`,
`player2_publickey`) with each other and the tic-tac-toe program.

### Game Play

Both players poll the network, via a **TBD off-chain RPC API**, to read the
current game state from the `gamestate_publickey` memory page.

When the *Game status* field indicates it's their turn, the player issues a
*move* call transaction passing in the board position (1..9) that they want to
mark as X or O:
```
call(tictactoe_publickey, gamestate_publickey, 'move', position)
```
The program will reject the transaction if it was not signed by the player whose
turn it is.

The outcome of the *move* call is also observed by polling the current game state via
the **TBD off-chain RPC API**.

### Game Cancellation

At any time Player 1 may conclude the game by issuing:
```
call(tictactoe_publickey, gamestate_publickey, 'abort')
```
causing any remaining *rent* tokens assigned to the `gamestate_publickey` page
to be transferred back to Player 1 by the tic-tac-toe program.  Lastly, the
network recognizes the empty account and frees the `gamestate_publickey` memory
page.
