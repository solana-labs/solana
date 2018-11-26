# Programming Model

With the Solana runtime, we can execute on-chain programs concurrently, and
written in the client’s choice of programming language.

## Client interactions with Solana

<img alt="SDK tools" src="img/sdk-tools.svg" class="center"/>

As shown in the diagram above an untrusted client, creates a program in the
language of their choice, (i.e. C/C++/Rust/Lua), and compiles it with LLVM to a
position independent shared object ELF, targeting BPF bytecode, and sends it to
the Solana cluster. Next, the client sends messages to the Solana cluster,
which target that program. The Solana runtime loads the previously submitted
ELF and passes it the client's message for interpretation.

## Persistent Storage

Solana supports several kinds of persistent storage, called *accounts*:

1. Executable
2. Owned by a client
3. Owned by a program
4. Credit-only

All accounts are identified by public keys and may hold arbirary data.
When the client sends messages to programs, it requests access to storage
using those keys. The runtime loads the account data and passes it to the
program. The runtime also ensures accounts aren't written to if not owned
by the client or program. Any writes to credit-only accounts are discarded
unless the write was to credit tokens. Any user may credit other accounts
tokens, regardless of account permission.
