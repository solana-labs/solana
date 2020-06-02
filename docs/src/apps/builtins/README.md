# Builtin Programs

Solana contains a small handful of builtin programs, which are required to run
validator nodes. Unlike third-party programs, the builtin programs are part of
the validator implementation and can be upgraded as part of cluster upgrades.
Upgrades may occur to add features, fix bugs, or improve performance. Interface
changes to individual instructions should rarely, if ever, occur. Instead, when
change is needed, new instructions are added and previous ones are marked
deprecated. Apps can upgrade on their own timeline without concern of breakages
across upgrades.

The builtin programs include the System, Config, Stake, Vote, and BPFLoader
programs. For each, we provide the program ID and describe each supported
instruction. A transaction can mix and match instructions from different
programs, as well include instructions from third-party programs.

## System Program

Create accounts and transfer lamports between them

* Program ID: `11111111111111111111111111111111`
* Instructions: [SystemInstruction](https://docs.rs/solana-sdk/LATEST_SOLANA_RELEASE_VERSION/solana_sdk/system_instruction/enum.SystemInstruction.html)

## Config Program

Add configuration data to the chain and the list of public keys that are permitted to modify it

* Program ID: `Config1111111111111111111111111111111111111`
* Instructions: [config_instruction](https://docs.rs/solana-config-program/LATEST_SOLANA_RELEASE_VERSION/solana_config_program/config_instruction/index.html)

Unlike the other programs, the Config program does not define any individual
instructions. It has just one implicit instruction, a "store" instruction. Its
instruction data is a set of keys that gate access to the account, and the
data to store in it.

## Stake Program

Create stake accounts and delegate it to validators

* Program ID: `Stake11111111111111111111111111111111111111`
* Instructions: [StakeInstruction](https://docs.rs/solana-stake-program/LATEST_SOLANA_RELEASE_VERSION/solana_stake_program/stake_instruction/enum.StakeInstruction.html)

## Vote Program

Create vote accounts and vote on blocks

* Program ID: `Vote111111111111111111111111111111111111111`
* Instructions: [VoteInstruction](https://docs.rs/solana-vote-program/LATEST_SOLANA_RELEASE_VERSION/solana_vote_program/vote_instruction/enum.VoteInstruction.html)

## BPF Loader

Add programs to the chain.

* Program ID: `BPFLoader1111111111111111111111111111111111`
* Instructions: [LoaderInstruction](https://docs.rs/solana-sdk/LATEST_SOLANA_RELEASE_VERSION/solana_sdk/loader_instruction/enum.LoaderInstruction.html)

The BPF Loader marks itself as its "owner" of the executable account it
creates to store your program. When a user invokes an instruction via a
program ID, the Solana runtime will load both your executable account and its
owner, the BPF Loader. The runtime then passes your program to the BPF Loader
to process the instruction.