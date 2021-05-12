---
title: "Developing with C"
---

Solana는 C 및 C ++ 프로그래밍 언어를 사용하여 온 체인 프로그램 작성을 지원합니다.

## 프로젝트 레이아웃

다음과 같이C 프로젝트는 배치된다

```
/src/<program name>
/makefile
```

:`/ SRC / <프로그램 이름> / 메이크`은`makefile`은

```bash
OUT_DIR := <path to place to resulting shared object>
include ~/.local/share/solana/install/active_release/bin/sdk/bpf/c/bpf.mk
```

다음을 포함한다

:`bash는 OUT_DIR을 = <경로 곳으로 공유 결과적으로 개체> 포함 거치지 만 ~ / .local / share / 솔라 / 설치 / active_release / 빈 / SDK / BPF / C / bpf.mk`BPF를-SDK

## 빌드 방법

위에서 지정한 정확한 장소에 있지만, 설치 환경에 따라 경우하지 않을 수 있습니다 \[How to Build\] (# how-to-build) 그러면됩니다.

- 프로그램의 진입 점 입력 데이터가 역 직렬화되는 구조.
- / typedef struct { SolAccountInfo _ ka; / \*\* SolAccountInfo 배열에 대한 포인터, 이미 SolAccountInfo 배열을 가리켜 야 함 _ / uint64_t ka_num; / **`ka`의 SolAccountInfo 항목 수 _ / const uint8_t _ data; / ** 명령어 데이터에 대한 포인터 _ / uint64_t data_len; / \*\* 명령어 데이터의 길이 (바이트) _ / const SolPubkey _ program_id; / \*\* 현재 실행중인 프로그램의 program_id _ / } SolParameters;

C 프로그램의 예는 \[helloworld\] (https://github.com/solana-labs/example-helloworld/tree/master/src/program-c)를 참조하세요.

````bash
그런 다음 make를 사용하여 빌드합니다
.```bash
make -C
````

## 데이터 형식

테스트 방법 Solana는 \[Criterion\] (https://github.com/Snaipe/Criterion) 테스트 프레임 워크 및 테스트를 사용합니다. 프로그램이 빌드 될 때마다 실행됩니다 \[How to Build\] (# how-to-build)].

To add tests, create a new file next to your source file named `test_<program name>.c` and populate it with criterion test cases. 예제는 \[helloworld C 테스트\\] (https://github.com/solana-labs/example-helloworld/blob/master/src/program-c/src/helloworld/test_helloworld.c) 또는 \[Criterion docs \] (https://criterion.readthedocs.io/en/master)는 테스트 케이스 작성 방법에 대한 정보를 참조하십시오.

## 예제

Programs export a known entrypoint symbol which the Solana runtime looks up and calls when invoking a program. Solana supports multiple [versions of the BPF loader](overview.md#versions) and the entrypoints may vary between them. Programs must be written for and deployed to the same loader. For more details see the [overview](overview#loaders).

\[helloworld의 역 직렬화 기능 사용\] (https://github.com/solana-labs/example-helloworld/blob/bc0b25c0ccebeff44df9760ddb97011558b7d234/src/program-c/src/helloworld/helloworld.c#L43) 참조 ).

\[helloworld의 역 직렬화 기능 사용\] (https://github.com/solana-labs/example-helloworld/blob/bc0b25c0ccebeff44df9760ddb97011558b7d234/src/program-c/src/helloworld/helloworld.c#L43) 참조 ).

```c
extern uint64_t entrypoint(const uint8_t *input)
```

이 진입 점은 직렬화 된 프로그램 매개 변수 (프로그램 ID, 계정, 명령어 데이터 등)를 포함하는 일반 바이트 배열을 사용합니다. 매개 변수를 deserialize하기 위해 각 로더에는 자체 \[helper function\] (# Serialization)이 포함되어 있습니다.

로더가 프로그램 입력을 직렬화하는 방법에 대한 자세한 내용은 \[Input Parameter Serialization\] (overview.md # input-parameter-serialization) 문서에서 찾을 수 있습니다.

### 직렬화

로더의 직렬화 도우미 함수 [SolParameters (https://github.com/solana-labs/solana/blob/8415c22b593f164020adc7afe782e8041d756ddf/sdk/bpf/c/inc/solana_sdk.h#L276) 구조를

로더의 직렬화 도우미 함수 [SolParameters (https://github.com/solana-labs/solana/blob/8415c22b593f164020adc7afe782e8041d756ddf/sdk/bpf/c/inc/solana_sdk.h#L276) 구조를

- [BPF Loader deserialization](https://github.com/solana-labs/solana/blob/d2ee9db2143859fa5dc26b15ee6da9c25cc0429c/sdk/bpf/c/inc/solana_sdk.h#L304)
- [BPF Loader deprecated deserialization](https://github.com/solana-labs/solana/blob/8415c22b593f164020adc7afe782e8041d756ddf/sdk/bpf/c/inc/deserialize_deprecated.h#L25)

일부 프로그램에서 자체적으로 deserialzaiton을 수행하고 \[raw entrypoint\] (# program-entrypoint)의 자체 구현을 제공하여 수행 할 수 있습니다. 제공된 역 직렬화 함수는 프로그램이 수정할 수있는 변수 (램 포트, 계정 데이터)에 대한 직렬화 된 바이트 배열에 대한 참조를 다시 유지합니다. 그 이유는 반환시 로더가 해당 수정 사항을 읽고 커밋 할 수 있기 때문입니다. 프로그램이 자체 역 직렬화 기능을 구현하는 경우 프로그램이 커밋하려는 수정 사항을 입력 바이트 배열에 다시 기록해야합니다.

각 로더는 프로그램의 입력 매개 변수를 C 유형으로 역 직렬화하는 도우미 함수를 제공합니다 .-\[BPF 로더 역 직렬화\] (https://github.com/solana-labs/solana/blob/d2ee9db2143859fa5dc26b15ee6da9c25cc0429c/sdk/bpf/c/inc/solana_sdk .h # L304) -\[BPF Loader deprecated deserialization\] (https://github.com/solana-labs/solana/blob/8415c22b593f164020adc7afe782e8041d756ddf/sdk/bpf/c/inc/deserialize_deprecated.h#L25)

## Data Types

Currently there are two supported loaders [BPF Loader](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/bpf_loader.rs#L17) and [BPF loader deprecated](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/bpf_loader_deprecated.rs#L14)

```c
/**
 * Structure that the program's entrypoint input data is deserialized into.
 */
typedef struct {
  SolAccountInfo* ka; /** Pointer to an array of SolAccountInfo, must already
                          point to an array of SolAccountInfos */
  uint64_t ka_num; /** Number of SolAccountInfo entries in `ka` */
  const uint8_t *data; /** pointer to the instruction data */
  uint64_t data_len; /** Length in bytes of the instruction data */
  const SolPubkey *program_id; /** program_id of the currently executing program */
} SolParameters;
```

'ka'는 명령에서 참조하고 \[SolAccountInfo\] (https://github.com/solana-labs/solana/blob/8415c22b593f164020adc7afe782e8041d756ddf/sdk/bpf/c/inc)로 표시되는 정렬 된 계정 배열입니다. /solana_sdk.h#L173) 구조. 배열에서 계정의 위치는 의미를 나타냅니다. 예를 들어 램프 포트를 전송할 때 명령은 첫 번째 계정을 소스로 정의하고 두 번째 계정을 대상으로 정의 할 수 있습니다.

`SolAccountInfo` 구조의 멤버는`lamports` 및`data`를 제외하고 읽기 전용입니다. 둘 다 \[런타임 시행 정책\] (developing / programming-model / accounts.md # policy)에 따라 프로그램에 의해 수정 될 수 있습니다. 명령어가 동일한 계정을 여러 번 참조하는 경우 배열에 중복 된 'SolAccountInfo'항목이있을 수 있지만 둘 다 원래 입력 바이트 배열을 다시 가리 킵니다. 프로그램은 동일한 버퍼에 대한 읽기 / 쓰기가 겹치지 않도록 이러한 경우를 섬세하게 처리해야합니다. 프로그램이 자체 역 직렬화 기능을 구현하는 경우 중복 계정을 적절하게 처리하도록주의해야합니다.

`data` is the general purpose byte array from the [instruction's instruction data](developing/programming-model/transactions.md#instruction-data) being processed.

`program_id` is the public key of the currently executing program.

## 예산 계산

C programs can allocate memory via the system call [`calloc`](https://github.com/solana-labs/solana/blob/c3d2d2134c93001566e1e56f691582f379b5ae55/sdk/bpf/c/inc/solana_sdk.h#L245) or implement their own heap on top of the 32KB heap region starting at virtual address x300000000. The heap region is also used by `calloc` so if a program implements their own heap it should not also call `calloc`.

## 로깅

The [debugging](debugging.md#logging) section has more information about working with program logs.

- [`sol_log(const char*)`](https://github.com/solana-labs/solana/blob/d2ee9db2143859fa5dc26b15ee6da9c25cc0429c/sdk/bpf/c/inc/solana_sdk.h#L128)
- [`sol_log_64(uint64_t, uint64_t, uint64_t, uint64_t, uint64_t)`](https://github.com/solana-labs/solana/blob/d2ee9db2143859fa5dc26b15ee6da9c25cc0429c/sdk/bpf/c/inc/solana_sdk.h#L134)

Use the system call [`sol_log_compute_units()`](https://github.com/solana-labs/solana/blob/d3a3a7548c857f26ec2cb10e270da72d373020ec/sdk/bpf/c/inc/solana_sdk.h#L140) to log a message containing the remaining number of compute units the program may consume before execution is halted

## Compute Budget

See [compute budget](developing/programming-model/../../../programming-model/runtime.md/#compute-budget) for more information.

See [compute budget](developing/programming-model/runtime.md#compute-budget) for more information.

## ELF Dump

The BPF shared object internals can be dumped to a text file to gain more insight into a program's composition and what it may be doing at runtime. The dump will contain both the ELF information as well as a list of all the symbols and the instructions that implement them. Some of the BPF loader's error log messages will reference specific instruction numbers where the error occurred. These references can be looked up in the ELF dump to identify the offending instruction and its context.

To create a dump file:

```bash
$ cd <program directory>
$ make dump_<program name>
```

## 예제

The [Solana Program Library github](https://github.com/solana-labs/solana-program-library/tree/master/examples/c) repo contains a collection of C examples
