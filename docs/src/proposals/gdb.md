# RFC: Porting GDB to BPF

## Problem

[rpbf](https://github.com/solana-labs/rbpf) doesn't provide any debugging tools to debug BPF-interpreted programs, so for the most part the user can only use print statements. This makes it quite difficult for them to fix pernicious memory-related bugs in code that does stuff like pointer arithmetic that's often present in solana programs.

It would be awesome if we had the ability to attatch a debugger to BPF, ideally GDB because the user is probably already familiar with it.

## Proposed Solution

Implement a remote target for GDB, where the debugger and the VM communicate using any of the communication channels supported by GDB. The VM itself would have an additional module acts in a similar manner to a GDB server. This feature should be conditionally-compled in a way that eliminates any performance impact on the VM when the debugger is not being used.

Using a remote is much less involved than trying to implement a gdb architecture definition, which requires lot of details that our rBPF VM doesn't care or know about. It also has the advantage that it keeps the debugger maximally isolated from the VM, which should make it much easier to minimize the performance impact when the debugger isn't attached. Depending on how this is setup, this would also give the user the ability to debug a VM instance running on a remote machine without doing anything differently from debugging a standalone VM instance running on the same machine.

## Implementation

1. Add an interface to the eBPF VM that exposes `ptrace`-like functionality that GDB can use to do things that GDB needs to do, including those listed below. This could be done by extending the already-extant [`Tracer`](https://github.com/solana-labs/rbpf/blob/126b0c84cea9005fd684fefe587fc7edde1aa5eb/src/vm.rs#L265) struct or creating a new one, along with some conditionally compiled code in the VM itself for starting / stopping / continuing the [main execution loop](https://github.com/solana-labs/rbpf/blob/126b0c84cea9005fd684fefe587fc7edde1aa5eb/src/vm.rs#L608).

   - inspecting register state and program memory
   - software breakpoints/interrupts
   - single-step
   - stack frame unwinding
   - prologue inspection/skipping

2. [Implement a remote GDB stub](http://davis.lbl.gov/Manuals/GDB/gdb_17.html#SEC140) using interface from 1 via FFI and implements GDB's remote debugging protocol using code that's already been written by previous stub implementations. The protocol can use TCP/IP, pipes (stdin/stdout), Unix domain socket, or even a serial bus if someone's trying to port the solana client to something wacky like a PS4. Normally this is implemented as a stub in C that redirects interrupt handlers to a method that sends packets to the debugger, but since the VM is entirely in software we can skip that and just recv/send packets in the VM itself whenever we reach a software breakpoint or other exceptions.