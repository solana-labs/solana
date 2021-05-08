---
title: "用Rust开发"
---

Solana 支持使用[Rust](https://www.rust-lang.org/) 编程语言编写链上的程序。

## 项目布局 {#project-layout}

Solana Rust程序遵循典型的[Rust项目布局](https://doc.rust-lang.org/cargo/guide/project-layout.html)：

```
/inc/
/src/
/Cargo.toml
```

但也必须包括：
```
/Xargo.toml
```
必须包含：
```
[target.bpfel-unknown-unknown.dependencies.std]
features = []
```

Solana Rust 程序可能会直接依赖于对方，以便在进行 [交叉程序调用](developing/programming-model/calling-between-programs.md#cross-program-invocations)时获得指令协助。 这样做时，重要的是不要拉入依赖程序的入口点符号，因为它们可能与程序本身的符号冲突。  为避免这种情况，程序应在 `Cargo.toml` 中定义一个 ` exclude_entrypoint `功能，并使用它来排除入口点。

- [定义特性](https://github.com/solana-labs/solana-program-library/blob/a5babd6cbea0d3f29d8c57d2ecbbd2a2bd59c8a9/token/program/Cargo.toml#L12)
- [排除入口点](https://github.com/solana-labs/solana-program-library/blob/a5babd6cbea0d3f29d8c57d2ecbbd2a2bd59c8a9/token/program/src/lib.rs#L12)

然后，当其他程序将此程序作为依赖项包括在内时，它们应该使用`exclude_entrypoint`功能来实现这一点。
- [不将入口点包含在内](https://github.com/solana-labs/solana-program-library/blob/a5babd6cbea0d3f29d8c57d2ecbbd2a2bd59c8a9/token-swap/program/Cargo.toml#L19)

## 项目依赖关系 {#project-dependencies}

至少，Solana Rust程序必须引入[solana-program](https://crates.io/crates/solana-program)。

Solana BPF程序具有某些[限制](#Restrictions)，可能会阻止将某些箱体作为依赖项包含进来或需要特殊处理。

例如：
- 要求架构的箱体（Crates）是官方工具链支持箱体的子集。  除非解决了这个问题，并且没有将BPF添加到那些体系结构检查中，否则没有解决方法。
- 箱体可能取决于Solana确定性程序环境中不支持的`rand`。  要包含`rand`相关的箱体，请参考[在 Rand 开发](#depending-on-rand)。
- 即使程序本身未包含堆栈溢出代码，箱体也可能会使堆栈溢出。  有关的更多信息，请参见[Stack](overview.md#stack)。

## 如何开发 {#how-to-build}

首先设置环境：
- 从https://rustup.rs/安装最新的Rust稳定版本
- 从https://docs.solana.com/cli/install-solana-cli-tools安装最新的Solana命令行工具

正常的cargo构建可用于针对您的主机构建程序，该程序可用于单元测试：

```bash
$ cargo build
```

要为可部署到集群的Solana BPF目标构建一个特定的程序，例如SPL代币，请执行以下操作：

```bash
$ cd <the program directory>
$ cargo build-bpf
```

## 如何测试 {#how-to-test}

通过直接行使程序功能，可以通过传统的`cargo test`机制对Solana程序进行单元测试。

为了帮助在更接近实时集群的环境中进行测试，开发人员可以使用[`program-test`](https://crates.io/crates/solana-program-test)箱体。  `程序测试`箱体将启动运行时的本地实例，并允许测试发送多个事务，同时在测试期间保持状态。

有关更多信息，请参见[在sysvar示例中测试](https://github.com/solana-labs/solana-program-library/blob/master/examples/rust/sysvar/tests/functional.rs)，来学习如何包含一条指令syavar帐户由程序发送和处理。

## 程序入口点 {#project-entrypoint}

程序导出一个已知的入口点符号，在调用程序时，Solana运行时将查找并调用该入口点符号。  Solana支持多个[BPF加载程序版本](overview.md#versions)，它们之间的入口点可能会有所不同。 程序必须为相同的加载器编写并部署。  有关更多详细信息，请参见[概览](overview#loaders)。

当前有两个受支持的加载器：[BPF加载器](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/bpf_loader.rs#L17)和[已弃用BFT加载器](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/bpf_loader_deprecated.rs#L14)。

它们都有相同的原始入口点定义，以下是运行时查找和调用的原始符号：

```rust
#[no_mangle]
pub unsafe extern "C" fn entrypoint(input: *mut u8) -> u64;
```

该入口点采用通用字节数组，其中包含序列化的程序参数(程序ID，帐户，指令数据等)。  为了反序列化参数，每个加载程序都包含其自己的包装宏，该宏导出原始入口点，反序列化参数，调用用户定义的指令处理函数并返回结果。

您可以在此处找到入口点宏：
- [BPF加载程序的入口点宏](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/entrypoint.rs#L46)
- [BPF 加载器不推荐使用的入口点宏](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/entrypoint_deprecated.rs#L37)

入口点宏调用的程序定义的指令处理功能必须具有以下形式：

```rust
pub type ProcessInstruction =
    fn(program_id: &Pubkey, accounts: &[AccountInfo], instruction_data: &[u8]) -> ProgramResult;
```

请参阅 [使用入口点的简单实例](https://github.com/solana-labs/example-helloworld/blob/c1a7247d87cd045f574ed49aec5d160aefc45cf2/src/program-rust/src/lib.rs#L15)，来看看它们是如何配合使用的。

### 参数反序列化 {#parameter-deserialization}

每个加载程序都提供一个帮助程序功能，该功能将程序的输入参数反序列化为Rust类型。  入口点宏会自动调用反序列化帮助器：
- [BPF加载器反序列化](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/entrypoint.rs#L104)
- [BPF 加载器已弃用的反序列化](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/entrypoint_deprecated.rs#L56)

某些程序可能希望自己执行反序列化，并且可以通过提供其自己的[原始入口点](#program-entrypoint)实现来实现。 请注意，提供的反序列化功能会将引用保留回序列化字节数组，以引用允许程序修改的变量(lamport，帐户数据)。  这样做的原因是，在返回时，加载程序将读取这些修改，以便可以将其提交。  如果程序实现其自己的反序列化功能，则需要确保将程序希望进行的所有修改都写回到输入字节数组中。

有关加载程序如何序列化程序输入的详细信息，请参见[Input Parameter Serialization](overview.md#input-parameter-serialization)文档。

### 数据类型 {#data-types}

加载程序的入口点宏使用以下参数调用程序定义的指令处理器功能：

```rust
program_id: &Pubkey,
accounts: &[AccountInfo],
instruction_data: &[u8]
```

程序ID是当前正在执行的程序的公钥。

帐户是指令引用的帐户的有序切片，并表示为[AccountInfo](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/account_info.rs#L10)结构。  帐户在数组中的位置表示其含义，例如，在转移lamports时，一条指令可以将第一个帐户定义为源，将第二个帐户定义为目的地。

`AccountInfo`结构的成员是只读的，但`lamports`和`data`除外。  程序都可以根据[runtime执行策略](developing/programming-model/accounts.md#policy)对两者进行修改。  这两个成员都受`RustRefCell`构造的保护，因此必须借用它们以对其进行读写。  这样做的原因是它们都指向原始输入字节数组，但是帐户片中可能有多个条目指向同一帐户。  使用`RefCell`确保程序不会通过多个`AccountInfo`结构意外地对相同的基础数据执行重叠的读/写操作。  如果程序实现其自己的反序列化功能，则应注意适当地处理重复帐户。

指令数据是正在处理的[指令的指令数据](developing/programming-model/transactions.md#instruction-data)中的通用字节数组。

## 堆（Heap）{#heap}

Rust程序通过定义自定义[`global_allocator`](https://github.com/solana-labs/solana/blob/8330123861a719cd7a79af0544617896e7f00ce3/sdk/program/src/entrypoint.rs#L50)直接实现堆。

程序可以根据其特定需求实现自己的`global_allocator`。 相关的更多信息，请参考[自定义heap示例](#examples)。

## 限制 {#restrictions}

链上Rust程序支持Rust的大多数libstd，libcore和liballoc，以及许多第三方包装箱。

由于这些程序在资源受限的单线程环境中运行，因此存在一定的局限性，并且必须是确定性的：

- 无法访问
  - `rand`
  - `std::fs`
  - `std::net`
  - `std::os`
  - `std::future`
  - `std::net`
  - `std::process`
  - `std::sync`
  - `std::task`
  - `std::thread`
  - `std::time`
- 有限的访问权：
  - `std::hash`
  - `std::os`
- 二进制代码在周期和调用深度上在计算上都非常昂贵，应该尽量避免。
- 应该避免字符串格式化，因为它在计算上也很昂贵。
- 不支持 `println!`，`print!`，应该使用Solana [logging helpers](#logging)。
- 运行时对程序在一条指令的处理过程中可以执行的指令数施加了限制。  相关的更多信息，请参见[计算预算](developing/programming-model/runtime.md#compute-budget)。

## 在Rand开发 {#depending-on-rand}

程序必须确定性地运行，因此不能使用随机数。 有时，即使程序不使用任何随机数功能，程序也可能依赖于自己的`rand`。 如果程序依赖于`rand`，则编译将失败，因为对Solana没有对`get-random`进行支持。 报错通常如下所示：

```
error: target is not supported, for more information see: https://docs.rs/getrandom/#unsupported-targets
   --> /Users/jack/.cargo/registry/src/github.com-1ecc6299db9ec823/getrandom-0.1.14/src/lib.rs:257:9
    |
257 | /         compile_error!("\
258 | |             target is not supported, for more information see: \
259 | |             https://docs.rs/getrandom/#unsupported-targets\
260 | |         ");
    | |___________^
```

要解决此依赖性问题，请将以下依赖性添加到程序的`Cargo.toml`中：

```
getrandom = { version = "0.1.14", features = ["dummy"] }
```

## 日志 {#logging}

Rust的`println`宏在计算上很昂贵，不被支持。  而是提供了辅助宏[`msg!`](https://github.com/solana-labs/solana/blob/6705b5a98c076ac08f3991bb8a6f9fcb280bf51e/sdk/program/src/log.rs#L33)。

`msg!` 有两种形式：

```rust
msg!("A string");
```
或者
```rust
msg!(0_64, 1_64, 2_64, 3_64, 4_64);
```

两者都将输出结果到程序日志。  如果程序愿意，他们可以使用`format!`来模拟`println!`：

```rust
msg!("Some variable: {:?}", variable);
```

[debugging](debugging.md#logging)章节提供了有关使用程序日志的更多信息，[Rust示例](#examples)包含一个日志记录示例。

## 恐慌（Panicking）{#panicking}

默认情况下，Rust 的`panic!`、`assert!`和内部恐慌结果被打印到[程序日志](debugging.md#logging)。

```
INFO  solana_runtime::message_processor] Finalized account CGLhHSuWsp1gT4B7MY2KACqp9RUwQRhcUFfVSuxpSajZ
INFO  solana_runtime::message_processor] Call BPF program CGLhHSuWsp1gT4B7MY2KACqp9RUwQRhcUFfVSuxpSajZ
INFO  solana_runtime::message_processor] Program log: Panicked at: 'assertion failed: `(left == right)`
      left: `1`,
     right: `2`', rust/panic/src/lib.rs:22:5
INFO  solana_runtime::message_processor] BPF program consumed 5453 of 200000 units
INFO  solana_runtime::message_processor] BPF program CGLhHSuWsp1gT4B7MY2KACqp9RUwQRhcUFfVSuxpSajZ failed: BPF program panicked
```

### 自定义恐慌处理器 {#custom-panic-handler}

程序可以通过提供自己的实现来覆盖默认的紧急处理程序。

首先在程序的`Cargo.toml`中定义`custom-panic`功能。

```toml
[features]
default = ["custom-panic"]
custom-panic = []
```

然后提供应急处理程序的自定义实现：

```rust
#[cfg(all(feature = "custom-panic", target_arch = "bpf"))]
#[no_mangle]
fn custom_panic(info: &core::panic::PanicInfo<'_>) {
    solana_program::msg!("program custom panic enabled");
    solana_program::msg!("{}", info);
}
```

在上面的代码段中，显示了默认的实现，但是开发人员可以用更适合他们需求的东西代替它。

默认情况下，支持完整的紧急消息的副作用之一是程序会产生将Rust的更多`libstd`实现引入程序共享对象的代价。  典型的程序将已经引入了相当数量的`libstd`，并且可能不会注意到共享对象大小的增加。  但是那些通过避免使用`libstd`显式地试图变得很小的程序可能会产生很大的影响(~25kb)。  为了消除这种影响，程序可以为自己的自定义应急处理程序提供空的实现。

```rust
#[cfg(all(feature = "custom-panic", target_arch = "bpf"))]
#[no_mangle]
fn custom_panic(info: &core::panic::PanicInfo<'_>) {
    // Do nothing to save space
}
```

## 计算预算 {#compute-budget}

使用系统调用[`sol_log_compute_units()`](https://github.com/solana-labs/solana/blob/d3a3a7548c857f26ec2cb10e270da72d373020ec/sdk/program/src/log.rs#L102)]记录包含剩余编号的消息暂停执行之前程序可能消耗的计算单元数。

相关的更多信息，请参见[计算预算](developing/programming-model/runtime.md#compute-budget)。

## ELF转储 {#elf-dump}

可以将BPF共享对象的内部信息转储到文本文件中，以更深入地了解程序的组成及其在运行时的工作方式。  转储将包含ELF信息以及所有符号和实现它们的指令的列表。  一些BPF加载程序的错误日志消息将引用发生错误的特定指令号。 可以在ELF转储中查找这些引用，以标识有问题的指令及其上下文。

创建一个转储文件：

```bash
$ cd <program directory>
$ cargo build-bpf --dump
```

## 示例 {#examples}

[Solana 程序库github](https://github.com/solana-labs/solana-program-library/tree/master/examples/rust)代码库包含了Rust例子集合。
