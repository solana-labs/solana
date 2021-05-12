---
title: "러스트로 개발하기"
---

Solana는 \[Rust\] (https://www.rust-lang.org/) 프로그래밍 언어를 사용하여 온 체인 프로그램 작성을 지원합니다.

## 프로젝트 레이아웃

:솔라 녹 프로그램 (https://doc.rust-lang.org/cargo/guide/project-layout.html) 전형적인 [녹 프로젝트 레이아웃]을 따라

```
/inc/
/src/
/Cargo.toml
```

But must also include:

```
pub type ProcessInstruction =
    fn (program_id : & Pubkey, accounts : & [AccountInfo], instruction_data : & [u8])-> ProgramResult;
```

진입 점 매크로는 다음에서 찾을 수 있습니다 .-\[BPF 로더의 진입 점 매크로\] (https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/entrypoint.rs#L46)-\[BPF 로더 deprecated의 진입 점 매크로\] (https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/entrypoint_deprecated.rs#L37)

```
[target.bpfel-unknown-unknown.dependencies.std]
features = []
```

Solana Rust programs may depend directly on each other in order to gain access to instruction helpers when making [cross-program invocations](developing/programming-model/calling-between-programs.md#cross-program-invocations). When doing so it's important to not pull in the dependent program's entrypoint symbols because they may conflict with the program's own. To avoid this, programs should define an `exclude_entrypoint` feature in `Cargo.toml` and use to exclude the entrypoint.

- [Define the feature](https://github.com/solana-labs/solana-program-library/blob/a5babd6cbea0d3f29d8c57d2ecbbd2a2bd59c8a9/token/program/Cargo.toml#L12)
- [Exclude the entrypoint](https://github.com/solana-labs/solana-program-library/blob/a5babd6cbea0d3f29d8c57d2ecbbd2a2bd59c8a9/token/program/src/lib.rs#L12)

\[helloworld의 진입 점 사용\] (https://github.com/solana-labs/example-helloworld/blob/c1a7247d87cd045f574ed49aec5d160aefc45cf2/src/program-rust/src/lib.rs#L15) 참조 어떻게 조화를 이루는 지.

- [Include without entrypoint](https://github.com/solana-labs/solana-program-library/blob/a5babd6cbea0d3f29d8c57d2ecbbd2a2bd59c8a9/token-swap/program/Cargo.toml#L19)

## 힙

매개 변수 역 직렬화

Solana BPF programs have some [restrictions](#restrictions) that may prevent the inclusion of some crates as dependencies or require special handling.

예를

- Crates that require the architecture be a subset of the ones supported by the official toolchain. There is no workaround for this unless that crate is forked and BPF added to that those architecture checks.
- Crates may depend on `rand` which is not supported in Solana's deterministic program environment. To include a `rand` dependent crate refer to [Depending on Rand](#depending-on-rand).
- Crates may overflow the stack even if the stack overflowing code isn't included in the program itself. For more information refer to [Stack](overview.md#stack).

## 제한 사항

로더가 프로그램 입력을 직렬화하는 방법에 대한 자세한 내용은 \[Input Parameter Serialization\] (overview.md # input-parameter-serialization) 문서에서 찾을 수 있습니다.

- Install the latest Rust stable from https://rustup.rs/
- Install the latest Solana command-line tools from https://docs.solana.com/cli/install-solana-cli-tools

유형은로더의 엔트리 포인트 매크로 다음 매개 변수 프로그램 정의 명령어 처리 기능을 호출

```bash
$ cargo build
```

'녹 program_id`&Pubkey가 차지한다 : [AccountInfo의]은 instruction_data가있다 : [U8]가``프로그램

```bash
# [cfg (all (feature = "custom-panic", target_arch = "bpf"))]
# [no_mangle]
fn custom_panic (info : & core :: panic :: PanicInfo < '_>) {
    // 공간 절약을 위해 아무것도하지 마십시오
}
```

## Rand에 따라

ID가 인 현재 실행중인 프로그램의 공개 키입니다.

To help facilitate testing in an environment that more closely matches a live cluster, developers can use the [`program-test`](https://crates.io/crates/solana-program-test) crate. The `program-test` crate starts up a local instance of the runtime and allows tests to send multiple transactions while keeping state for the duration of the test.

For more information the [test in sysvar example](https://github.com/solana-labs/solana-program-library/blob/master/examples/rust/sysvar/tests/functional.rs) shows how an instruction containing syavar account is sent and processed by the program.

## Program Entrypoint

Programs export a known entrypoint symbol which the Solana runtime looks up and calls when invoking a program. Solana supports multiple [versions of the BPF loader](overview.md#versions) and the entrypoints may vary between them. Programs must be written for and deployed to the same loader. For more details see the [overview](overview#loaders).

러스트 프로그램은 커스텀 [`global_allocator`] (https://github.com/solana-labs/solana/blob/8330123861a719cd7a79af0544617896e7f00ce3/sdk/program/src/entrypoint.rs#L50)을 정의하여 직접 힙을 구현합니다.

They both have the same raw entrypoint definition, the following is the raw symbol that the runtime looks up and calls:

```rust
#[no_mangle]
pub unsafe extern "C" fn entrypoint(input: *mut u8) -> u64;
```

이 진입 점은 직렬화 된 프로그램 매개 변수 (프로그램 ID, 계정, 명령어 데이터 등)를 포함하는 일반 바이트 배열을 사용합니다. 매개 변수를 역 직렬화하기 위해 각 로더에는 원시 진입 점을 내보내고, 매개 변수를 역 직렬화하고, 사용자 정의 명령 처리 함수를 호출하고, 결과를 반환하는 자체 래퍼 매크로가 포함되어 있습니다.

이러한 프로그램은 리소스가 제한된 단일 스레드 환경에서 실행되기 때문에 몇 가지 제한 사항이 있으며 결정적이어야합니다

- [BPF Loader's entrypoint macro](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/entrypoint.rs#L46)
- [BPF Loader deprecated's entrypoint macro](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/entrypoint_deprecated.rs#L37)

진입 점 매크로가 호출하는 프로그램 정의 명령 처리 함수는 다음과 같아야합니다. form :

```rust
pub type ProcessInstruction =
    fn(program_id: &Pubkey, accounts: &[AccountInfo], instruction_data: &[u8]) -> ProgramResult;
```

Refer to [helloworld's use of the entrypoint](https://github.com/solana-labs/example-helloworld/blob/c1a7247d87cd045f574ed49aec5d160aefc45cf2/src/program-rust/src/lib.rs#L15) as an example of how things fit together.

### 데이터

각 로더는 프로그램의 입력 매개 변수를 Rust 유형으로 역 직렬화하는 도우미 기능을 제공합니다. 진입 점 매크로는 자동으로 역 직렬화 도우미를 호출합니다 .-\[BPF 로더 역 직렬화\] (https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/entrypoint.rs#L104)-\[BPF 로더 deprecated deserialization\] (https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/entrypoint_deprecated.rs#L56)

- [BPF Loader deserialization](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/entrypoint.rs#L104)
- [BPF Loader deprecated deserialization](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/entrypoint_deprecated.rs#L56)

일부 프로그램은 자체 구현을 제공하여 직접 역 직렬화를 수행 할 수 있습니다. \[원시 진입 점\] (# 프로그램 진입 점)의. 제공된 역 직렬화 함수는 프로그램이 수정할 수있는 변수 (램 포트, 계정 데이터)에 대한 직렬화 된 바이트 배열에 대한 참조를 다시 유지합니다. 그 이유는 반환시 로더가 해당 수정 사항을 읽고 커밋 할 수 있기 때문입니다. 프로그램이 자체 역 직렬화 기능을 구현하는 경우 프로그램이 커밋하려는 수정 사항이 입력 바이트 배열에 다시 기록되는지 확인해야합니다.

Details on how the loader serializes the program inputs can be found in the [Input Parameter Serialization](overview.md#input-parameter-serialization) docs.

### Data Types

`msg!`에는 두 가지 형식이 있습니다

```rust
program_id: &Pubkey,
accounts: &[AccountInfo],
instruction_data: &[u8]
```

The program id is the public key of the currently executing program.

계정은 명령에서 참조하는 계정의 정렬 된 조각이며 \[AccountInfo\] (https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/account_info.rs#L10)로 표시됩니다. ) 구조. 배열에서 계정의 위치는 의미를 나타냅니다. 예를 들어 램프 포트를 전송할 때 명령은 첫 번째 계정을 소스로 정의하고 두 번째 계정을 대상으로 정의 할 수 있습니다.

`AccountInfo` 구조의 멤버는`lamports` 및`data`를 제외하고 읽기 전용입니다. 둘 다 \[런타임 시행 정책\] (developing / programming-model / accounts.md # policy)에 따라 프로그램에 의해 수정 될 수 있습니다. 이 두 멤버는 Rust`RefCell` 구조에 의해 보호되므로 읽고 쓰려면 빌려야합니다. 그 이유는 둘 다 원래 입력 바이트 배열을 가리 키지 만 동일한 계정을 가리키는 계정 슬라이스에 여러 항목이있을 수 있기 때문입니다. `RefCell`을 사용하면 프로그램이 여러`AccountInfo` 구조를 통해 동일한 기본 데이터에 대해 실수로 중복 읽기 / 쓰기를 수행하지 않도록합니다. 프로그램이 자체 역 직렬화 기능을 구현하는 경우 중복 계정을 적절하게 처리하도록주의해야합니다.

위의 스 니핏에는 기본 구현이 표시되어 있지만 개발자는이를 자신의 요구에 더 적합한 것으로 대체 할 수 있습니다.

## 예산 계산

온 체인 Rust 프로그램은 대부분의 Rust의 libstd, libcore, liballoc과 많은 타사 상자를 지원합니다.

프로그램특정 요구에 따라 자체`global_allocator`를 구현할 수 있습니다. 자세한 내용은 \[커스텀 힙 예제\] (# examples)를 참조하세요.

## ELF 덤프

On-chain Rust programs support most of Rust's libstd, libcore, and liballoc, as well as many 3rd party crates.

There are some limitations since these programs run in a resource-constrained, single-threaded environment, and must be deterministic:

- No access to
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
- Limited access to:
  - `std::hash`
  - `std::os`
- Bincode is extremely computationally expensive in both cycles and call depth and should be avoided
- String formatting should be avoided since it is also computationally expensive.
- `println!`,`print!`를 지원하지 않습니다. 대신 Solana \[logging helpers\] (# logging)를 사용해야합니다.
- -런타임은 하나의 명령어를 처리하는 동안 프로그램이 실행할 수있는 명령어 수를 제한합니다. See [computation budget](developing/programming-model/runtime.md#compute-budget) for more information.

## Depending on Rand

프로그램은 결정적으로 실행되도록 제한되므로 난수를 사용할 수 없습니다. 때때로 프로그램은 프로그램이 난수 기능을 사용하지 않더라도`rand`에 의존하는 상자에 의존 할 수 있습니다. 프로그램이`rand`에 의존하는 경우 Solana에 대한`get-random` 지원이 없기 때문에 컴파일이 실패합니다. 이 오류는 일반적으로 다음과 같이

````
https://docs.rs/getrandom/#unsupported-targets: 목표는 더 많은 정보를 참조하십시오 지원되지 않습니다
-&#062; /Users/jack/.cargo/registry/src / github.com-1ecc6299db9ec823 / getrandom-0.1.14 / src / lib.rs : 257 : 9
|
257 | / compile_error! ( "\
258 | | target is not supported, for more information see : \
259 | | https : //docs.rs/getrandom/#unsupported-targets \
260 | |");
| | ___________
^```프로그램의<code>Cargo.toml</code>에
````

에

```
수행합니다<code>getrandom
= {버전 = "0.1.14"를, 기능 = [
"더미"]}</code>
```

## Logging

Rust의`println!`매크로는 계산 비용이 많이 들고 지원되지 않습니다. 대신 도우미 매크로 [`msg!`] (https://github.com/solana-labs/solana/blob/6705b5a98c076ac08f3991bb8a6f9fcb280bf51e/sdk/program/src/log.rs#L33)가 제공됩니다.

[솔라 나 프로그램 라이브러리 GitHub의 (https://github.com/solana-labs/ solana-program-library / tree / master / examples / rust) repo에는 Rust 예제 모음이 포함되어 있습니다.

````rust
.```rust
msg! ( "문자열");
````

or

```rust
(0_64, 1_64, 2_64, 3_64, 4_64);
```

두 형식 모두 결과를 프로그램 로그에 출력합니다. 프로그램이 그렇게 원한다면 그들은`에 println을 에뮬레이션 할 수 있습니다``형식을 변수)!!!

`````rust
사용하여````녹
MSG ( "일부 변수 : {?}",
`````

[디버깅](debugging.md#logging) 섹션에서 프로그램 로그 작업에 대한 정보가 있으며 [러스트 예시](#examples)는 로깅 예시를 포함하고 있습니다.

## Panicking

Panicking Rust의`panic!`,`assert!`및 내부 패닉 결과는 기본적으로 \[프로그램 로그\] (debugging.md # logging)에 인쇄됩니다.

````
:```정보
solana_runtime :: message_processor] 안 확정이 CGLhHSuWsp1gT4B7MY2KACqp9RUwQRhcUFfVSuxpSajZ계정`(왼쪽
정보 solana_runtime :: message_processor] 전화 BPF 프로그램 CGLhHSuWsp1gT4B7MY2KACqp9RUwQRhcUFfVSuxpSajZ
당황에서 : '주장 실패 정보 solana_runtime :: message_processor] 프로그램 로그  ==
      오른쪽)`왼쪽`1`바로`2`
     ', 녹 / 공포 / SRC / lib.rs : 22 : 5
INFO solana_runtime :: message_processor] BPF 프로그램이 200000 개 단위CGLhHSuWsp1gT4B7MY2KACqp9RUwQRhcUFfVSuxpSajZ의5453  소비
정보 solana_runtime :: message_processor] BPF 프로그램 실패: BPF
프로그램은`당황``

### 커스텀 패닉 핸들러

프로그램은 자체 구현을 제공하여 기본 패닉 핸들러를 재정의 할 수 있습니다.
````

### Custom Panic Handler

The [debugging](debugging.md#logging) section has more information about working with program logs the [Rust examples](#examples) contains a logging example.

First define the `custom-panic` feature in the program's `Cargo.toml`

```toml
[features]
default = ["custom-panic"]
custom-panic = []
```

Then provide a custom implementation of the panic handler:

````rust
handler :

```rust
# [cfg (all (feature = "custom-panic", target_arch = "bpf"))]
# [no_mangle]
fn custom_panic (info : & core :: panic :: PanicInfo < '_>) {
    solana_program :: msg! ( "프로그램 사용자 정의 패닉 활성화 됨");
    solana_program :: msg! ( "{}", 정보);
}
````

In the above snippit, the default implementation is shown, but developers may replace that with something that better suits their needs.

기본적으로 전체 패닉 메시지를 지원하는 부작용 중 하나는 프로그램이 프로그램의 공유 객체에 Rust의`libstd` 구현을 더 많이 가져 오는 데 드는 비용이 발생한다는 것입니다. 일반적인 프로그램은 이미 상당한 양의 'libstd'를 가져오고 공유 객체 크기가 크게 증가하지 않을 수 있습니다. 그러나`libstd`를 피하여 명시 적으로 매우 작게 시도하는 프로그램은 상당한 영향을 미칠 수 있습니다 (~ 25kb). 이러한 영향을 제거하기 위해 프로그램은 빈 구현으로 자체 사용자 지정 패닉 처리기를 제공 할 수 있습니다.

```rust
#[cfg(all(feature = "custom-panic", target_arch = "bpf"))]
#[no_mangle]
fn custom_panic(info: &core::panic::PanicInfo<'_>) {
    // Do nothing to save space
}
```

## 트랜잭션 엔진

시스템 호출 [`sol_log_compute_units ()`] (https://github.com/solana-labs/solana/blob/d3a3a7548c857f26ec2cb10e270da72d373020ec/sdk/program/src/log 사용) .rs # L102) 실행이 중지되기 전에 프로그램이 사용할 수있는 컴퓨팅 단위의 남은 수를 포함하는 메시지를 기록하려면

See [compute budget](developing/programming-model/runtime.md#compute-budget) for more information.

## ELF 덤프

BPF 공유 객체 내부를 텍스트 파일로 덤프하여 프로그램의 구성과 런타임에 수행 할 수있는 작업에 대한 더 많은 통찰력을 얻을 수 있습니다. 덤프에는 ELF 정보와 모든 기호 목록과이를 구현하는 명령어가 모두 포함됩니다. BPF 로더의 오류 로그 메시지 중 일부는 오류가 발생한 특정 명령 번호를 참조합니다. 이러한 참조는 ELF 덤프에서 조회하여 문제가되는 명령어와 해당 컨텍스트를 식별 할 수 있습니다.

덤프 파일을 만들려면

```bash
$ cd <program directory>
$ cargo build-bpf --dump
```

## 예제

덤프 파일을 만들려면
