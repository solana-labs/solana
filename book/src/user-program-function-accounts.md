# User Program and Function Accounts

This design describes the user, program and function accounts in Solana.  The
three accounts define how programs interact with users, and how programs
functionality can be extended with functions.

## UserAccount

This kind of account stores the users tokens for each program.

```
type OwnerProgram = Pubkey;

struct UserAccount {
    tokens: HashMap<OwnerProgram, Token>, 
} 
```

The OwnerProgram is the address of the program that defines the rules for that
token.  Each user account must have some SystemProgram tokens to exist.

## ProgramToken 

The program token defines the program specific tokens for the user.

```
struct ProgramToken {
    /// tokens in the account
    pub tokens: u64,
    /// data held in this account
    pub data: Vec<u8>,
}
```
 
* token: The number of program specific tokens.  For the System program, these
represent lamports.

* data: The persistent state.

Accounts maintain a vector of the tokens.

## ProgramAccount 

Program accounts store the functions that define the rules of the program.

```
struct ProgramAccount {
    lamports: u64,
    functions: Set<Pubkey>,
}
```

## FunctionAccount 

Function accounts store the bytecode for the function, and the lamports to keep
the function in memory.

```
struct FunctionAccount {
    lamports: u64,
    bytecode: Vec<u8>,
}
```

For a function to succeed the state transition, it must be in the set of
functions in the tokens OwnerProgram.
