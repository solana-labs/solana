# Solana Scripts

This architecture describes how the Pipeline runtime supports scripts. 

In Solana programs have absolute control over the state assigned to them.  The
goal of this architecture is provide a simple way for on-chain programs to
interact with other programs without violating the state ownership guarantees.

Contract composition has led to multiple multi-million dollar bugs in Ethereum

* https://www.parity.io/security-alert-2/

## Scripts

The ideal solution to this problem is an infallible Trusted Third Party. For
example, users that want to bet on an outcome of an on-chain game of
Tic-Tac-Toe, transfer their tokens to the TTP, and the TTP transfers the prize
to the winner at the end of the game.

Scripts are on-chain programs that can act as a TTP.

### Signatures and Permissions

A TTP needs the ability to create pubkeys and generate signatures for those
keys.  At script creation time the loader program may authorize specific
pubkeys that the script and only the script can sign with.  To ensure that these
keys cannot be signed by the user, the addresses are derived from a sha256 of
the script pubkey and the key sequence number.

* `fn script::keypair_pubkey(key_index: u64) -> Pubkey`

Retrivies the script keypair at index `key_index`.  The pubkey of the keypair is
`sha256.hash(program_id).hash(key_index)`, and therefore has no real private
key.  This keypair can only be used to sign messages witn the `sign_instruction`
function by the script.

* `fn script::sign_instruction(ix: &mut Instruction, key_index: u64) -> ()`

Signs the message for the script pubkey that is generated with `key_index`.
Users can generate these keys locally and encode them into the instruction
vector.  During the script execution, the script will call `sign_instruction`
and set the `KeyedAccounts::is_signer` flag.

### `fn script::process_instruction(ix: Instruction) -> Result<(), InstructionError>`

This method is available to scripts to execute an instruction in the runtime.

### `LoaderInstruction::FinalizeScript`

`LoaderInstruction::FinalizeScript` designates that the loaded executable
bytecode is a script, and creates a new instance of the script. The difference
between scripts and programs is that script execution yields to external program
instructions, and scripts have the capability to sign.  `FinalizeScript` may be
called more than once on the same loaded bytecode to create unique instances of
scripts each with their own signing keys.

### Script Execution

During the script execution, calls to `process_instruction` yield, and the next
instruction to be processed is invoked.  Users know ahead of time which
instructions the script will generate, and if signatures are required for the
instructions.

The transaction invoked by the client must declare all the accounts that the
script will need up front and provide all the necessary client signatures, as
well as encode the instruction vector that the script will generate.  The latter
provides clear authorization for the script to take actions on behalf of the
client, since each instruction specifies the clients keys and explicit
signatures for each explicit instruction.  Users do not need to guess which
instructions the script will execute, and authorize each one explicitly.

Once the instruction is invoked, the script is resumed from the last point of
execution.

### Script Execution Example

```
enum BetOnTicTacToe {
	Initialize {amount: u64, game: Pubkey},
	Claim,
};

pub fn process_instruction(
    program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    data: &[u8],
) -> Result<(), InstructionError> {
    let cmd = deserialize(data)?;
    match cmd {
        case BetOnTicTacToe::Initialize{ amount, game} => {
            let script_key = script::keypair_pubkey(0);
            let from_alice = system_instruction::transfer(
                            keyed_accounts[1].key, //alice
                            script_key,
                            amount);

            //alice must have signed this instruction
            script::process_instruction(from_alice)?;
            let from_bob = system_instruction::transfer(
                            keyed_accounts[2].key, //bob
                            script_key,
                            amount);

            //bob must have signed this instruction
            script::process_instruction(from_bob)?;

            //save the game
            assert_eq!(script_key, keyed_accounts[3].key);
            assert_eq!(program_id, keyed_accounts[3].account.owner);
            serialize(keyed_accounts[1].account.data, game)?;
        },
        case BetOnTicTacToe::TransferToWinner => {
            //script pubkey 0 is always the same
            let script_key = script::keypair_pubkey(0);

            assert_eq!(script_key, keyed_accounts[1].key);
            let game_key = deserialize(&keyed_accounts[1].account.data)?;

            assert_eq!(game_key, keyed_accounts[2].key);
            let game = deserialize(&keyed_accounts[2].account.data)?;

            assert!(game.is_over);

            //Ignoring ties for brevity 
            //transfer from the script to the winner of the game
            let mut to_winner = system_instruction::transfer(
                            script_key,
                            game.winner_key,
                            keyed_accounts[1].account.lamports);
            script::sign_instruction(&mut to_winner, 0);
            script::process_instruction(to_winner)?;
        },
    }
}

```

