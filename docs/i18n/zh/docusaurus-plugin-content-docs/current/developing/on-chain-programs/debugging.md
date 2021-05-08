---
title: "调试"
---

Solana程序在链上运行，因此在链外调试可能会很困难。 为了使调试程序更容易，开发人员可以编写单元测试以通过Solana运行时直接测试其程序的执行情况，或者运行允许RPC客户端与其程序进行交互的本地集群。

## 运行单元测试 {#running-unit-tests}

- [用Rust进行测试](developing-rust.md#how-to-test)
- [用C进行测试](developing-c.md#how-to-test)

## 记录 {#logging}

在程序执行期间，运行时，程序日志状态和错误消息均会出现。

有关如何从程序登录的信息，请参阅特定语言的文档：
- [从Rust程序记录日志](developing-rust.md#logging)
- [从C程序记录日志](developing-c.md#logging)

运行本地集群时，只要通过`RUST_LOG`日志掩码启用了日志，日志就会写入stdout。  从程序开发的角度来看，仅关注运行时和程序日志，而不关注其余的集群日志会有所帮助。  为了专注于程序特定的信息，建议使用以下日志掩码：

`export
RUST_LOG=solana_runtime::system_instruction_processor=trace,solana_runtime::message_processor=info,solana_bpf_loader=debug,solana_rbpf=debug`

直接来自程序(而不是runtime) 的日志消息将以以下形式显示：

`Program log: <user defined message>`

## 错误处理 {#error-handling}

可以通过事务错误传达的信息量是有限的，但是有很多可能的失败点。  以下是可能的故障点，有关预期发生哪些错误以及在何处获取更多信息的信息：
- BPF加载程序可能无法解析程序，这应该不会发生，因为加载程序已经对程序的帐户数据进行了_最终处理_。
  - `InstructionError::InvalidAccountData`将作为交易错误的一部分返回。
- BPF加载程序可能无法设置程序的执行环境
  - `InstructionError::Custom(0x0b9f_0001)`将作为交易错误的一部分返回。  "0x0b9f_0001"是[`VirtualMachineCreationFailed`](https://github.com/solana-labs/solana/blob/bc7133d7526a041d1aaee807b80922baa89b6f90/programs/bpf_loader/src/lib.rs#L44)的十六进制表示形式。
- BPF加载程序可能在程序执行过程中检测到致命错误(紧急情况，内存冲突，系统调用错误等)。
  - `InstructionError::Custom(0x0b9f_0002)`将作为交易错误的一部分返回。  "0x0b9f_0002"是[`VirtualMachineFailedToRunProgram`](https://github.com/solana-labs/solana/blob/bc7133d7526a041d1aaee807b80922baa89b6f90/programs/bpf_loader/src/lib.rs#L46)的十六进制表示。
- 程序本身可能返回错误
  - `InstructionError::Custom(<user defined value>)`将被返回。  “用户定义的值”不得与任何[内置运行时程序错误](https://github.com/solana-labs/solana/blob/bc7133d7526a041d1aaee807b80922baa89b6f90/sdk/program/src/program_error.rs#L87)相冲突 。 程序通常使用枚举类型来定义从零开始的错误代码，因此它们不会冲突。

如果出现`VirtualMachineFailedToRunProgram`错误，则将有关失败原因的详细信息写入[程序的执行日志](debugging.md#logging)。

例如，涉及堆栈的访问冲突将如下所示：

`BPF程序4uQeVj5tqViQh7yWWGStvkEG1Zmhx6uasJtWCJziofM失败：out of bounds
memory store (insn #615), addr 0x200001e38/8`

## 监控计算预算消耗 {#monitoring-compute-budget-consumption}

程序可以记录停止程序执行之前将允许的剩余计算单元数。  程序可以使用这些日志来包装希望分析的操作。

- [从Rust程序记录剩余的计算单元](developing-rust.md#compute-budget)
- [从C程序记录剩余的计算单元](developing-c.md#compute-budget)

有关更多信息，请参见[计算预算](developing/programming-model/runtime.md#compute-budget)。

## ELF转储 {#elf-dump}

可以将BPF共享对象的内部信息转储到文本文件中，以更深入地了解程序的组成及其在运行时的工作方式。

- [创建Rust程序的转储文件](developing-rust.md#elf-dump)
- [创建C程序的转储文件](developing-c.md#elf-dump)

## 指令追踪 {#instruction-tracing}

在执行期间，可以将运行时BPF解释器配置为记录每个执行的BPF指令的跟踪消息。  对于诸如精确指出导致内存访问冲突的运行时上下文之类的事情，这可能非常有用。

跟踪日志与[ELF转储](#elf-dump)一起可以提供更多参考(尽管跟踪会产生很多信息)。

要在本地集群中打开BPF解释器跟踪消息，请将`RUST_LOG`中的`solana_rbpf`级别配置为`trace`。  例如：

`export RUST_LOG=solana_rbpf=trace`
