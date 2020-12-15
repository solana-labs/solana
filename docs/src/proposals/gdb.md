# RFC: Porting GDB to BPF

## Problem

[rpbf](https://github.com/solana-labs/rbpf) doesn't provide any debugging tools to debug BPF-interpreted programs, so for the most part the user can only use print statements. This makes it quite difficult for them to fix pernicious memory-related bugs in code that does stuff like pointer arithmetic that's often present in solana programs.

It would be awesome if we had the ability to attatch a debugger to BPF, ideally GDB because the user is probably already familiar with it.

## Proposed Solution

I spent a little over a day poking around in GDB's source repo (git://sourceware.org/git/binutils-gdb.git) to get a handle on what this would entail and I think I have a pretty good idea of what it will take at a high level. However I don't have any experience hacking GDB internals like this, I've just used it a lot. So if anyone has better ideas and/or other opinions about how this should be done, please share.

1. Add an interface to the eBPF VM that exposes `ptrace`-like functionality that GDB can use to do the thing listed below. This could be done by extending the already-extant [`Tracer`](https://github.com/solana-labs/rbpf/blob/126b0c84cea9005fd684fefe587fc7edde1aa5eb/src/vm.rs#L265) struct or creating a new one, along with some conditionally compiled code in the VM itself for starting / stopping / continuing the [main execution loop](https://github.com/solana-labs/rbpf/blob/126b0c84cea9005fd684fefe587fc7edde1aa5eb/src/vm.rs#L608).

   - inspecting register state and program memory
   - software breakpoints
   - single-step
   - stack frame unwinding
   - prologue inspection/skipping

2. Implement gdb's "target-dependent" `gdbarch` interface using the provided interfaces. A few months ago [someone added very minimial definitons for eBPF to GDB](https://sourceware.org/git/?p=binutils-gdb.git;a=commitdiff;h=39791af2a2191a2f7765d7809ecedcd0442138bf), including basic stuff like register names, instruction format / size, word length, integer formats, etc, so we'd be able to build off of what's already written there.

This approach has the advantage that it keeps the debugger somewhat isolated from the VM (I'm pretty sure at least), so in theory it should be easy to change debugging behavior without messing too much with the VM itself.
