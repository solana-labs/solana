---
title: "Overview"
---

개발자들은 솔라나 블록체인에서 자신만의 프로그램을 작성하고 이를 배포할 수 있습니다.

프로그램 작성, 개발, 배포, 온체인 상호작용을 확인하기 위해 [Helloworld](examples.md#helloworld)는 매우 좋은 예시입니다.

## Berkley Packet Filter (BPF)

솔라나 온체인 프로그램은 [LLVM 컴파일러 인프라](https://llvm.org/)를 통해 컴파일되며 [Berkley Packet Filter (BPF)](https://en.wikipedia.org/wiki/Berkeley_Packet_Filter) 바이트코드의 파생을 담는 [Executable and Linkable Format (ELF)](https://en.wikipedia.org/wiki/Executable_and_Linkable_Format)로 컴파일 됩니다.

솔라나는 LLVM 컴파일러 인프라를 사용하기 때문에, 프로그램은 LLVM의 BPF 백엔드를 타겟팅 할 수 있는 어떠한 프로그래밍 언어로도 작성될 수 있습니다. 솔라나 프로그램 작성 지원 언어는 현재 러스트와 C/C++가 있습니다.

BPF는 가상머신 상에서, 또는 just-in-time 컴파일 네이티브 명령으로 실행될 수 있도록 효율적인 [명령 세트](https://github.com/iovisor/bpf-docs/blob/master/eBPF.md)를 제공합니다.

## 메모리 맵

솔라나 BPF 프로그램에 쓰이는 가상 메모리 주소는 고정되어 있으며 다음과 같습니다

- 프로그램 코드는 0x100000000로 시작합니다
- 스택 데이터는 0x200000000로 시작합니다
- 힙 데이터는 0x300000000로 시작합니다
- 프로그램 입력 패리미터는 0x400000000로 시작합니다

위의 가상 주소들은 시작 주소이지만 프로그램들에겐 메모리 맵 서브셋에 대한 접근도 주어집니다. 접근성이 없는 가상 주소로 프로그램이 접근하면 패닉이 발생하며 `AccessViolation`가 위반에 관련된 주소와 사이즈를 담는 에러가 반환됩니다.

## 스택

BPF는 변수 스택 포인터가 아닌 스택 프레임을 사용합니다. 각 스택 프레임은 4KB입니다.

만일 프로그램이 스택 프레임 사이즈를 위반한다면 컴파일러는 경고문을 오버런하며 다음과 같은 경고문을 띄웁니다.

For example: `Error: Function _ZN16curve25519_dalek7edwards21EdwardsBasepointTable6create17h178b3d2411f7f082E Stack offset of -30728 exceeded max offset of -4096 by 26632 bytes, please minimize large stack variables`

어떤 심볼이 스택을 넘어서는지는 메시지가 확인하지만 만일 러스트나 C, C++라면 mangle될 수 있습니다. [rustfilt](https://github.com/luser/rustfilt)를 사용하면 러스트 심볼을 demangle 할 수 있습니다. 위 경고문은 러스트 프로그램에서 나온 것입니다. 따라서 demangle 심볼명은:

```bash
$ rustfilt _ZN16curve25519_dalek7edwards21EdwardsBasepointTable6create17h178b3d2411f7f082E
curve25519_dalek::edwards::EdwardsBasepointTable::create
```

C++ demangle을 하려면 binutils의 `c++filt`를 사용하세요.

에러 대신 경고문이 뜨는 이유는 몇가지 디펜던시 크레이트가 프로그램이 쓰지 않고 스택 프레임 제한을 어기는 기능을 담고 있을수 있기 때문입니다. 만일 프로그램이 런타임에서 스택 사이즈를 위반한다면 `AccessViolation`에러가 보고됩니다.

BPF 스택 프레임의 가상 주소 범위는 0x200000000에서 시작됩니다.

## Call Depth

프로그램은 빠르게 동작할 수 있도록 제한되어 있으며 이를 실현하기 위해 프로그램의 호출 스택은 최고 64 프레임의 depth를 지닙니다.

## Heap

프로그램들은 C, 또는 러스트 `alloc` API를 통한 런타임 힙에 대한 접근을 가집니다. 빠른 얼로케이션을 활성화 하려면 간단한 32KB 범프 힙이 사용됩니다. 힙은 `free`나 `realloc`을 지원하지 않는 점을 유념하시기 바랍니다.

내부적으로 프로그램은 0x300000000로 시작하는 가상 주소를 지닌 32KB 메모리 지역에 대한 접근을 가지고 있으며 프로그램은 특정 요구사항에 따라 커스텀 힙을 적용할 수 있습니다.

- [러스트 프로그램 힙 사용](developing-rust.md#heap)
- [C 프로그램 힙 사용](developing-c.md#heap)

## 부동소수점 지원

Programs support a limited subset of Rust's float operations, if a program attempts to use a float operation that is not supported, the runtime will report an unresolved symbol error.

Float operations are performed via software libraries, specifically LLVM's float builtins. Due to be software emulated they consume more compute units than integer operations. In general, fixed point operations are recommended where possible.

The Solana Program Library math tests will report the performance of some math operations: https://github.com/solana-labs/solana-program-library/tree/master/libraries/math

To run the test, sync the repo, and run:

`$ cargo test-bpf -- --nocapture --test-threads=1`

Recent results show the float operations take more instructions compared to integers equivalents. Fixed point implementations may vary but will also be less then the float equivalents:

```
         u64   f32
Multipy    8   176
Divide     9   219
```

## Static Writable Data

프로그램 공유 객체는 쓰기가능한 공유 데이터를 지원하지 않습니다. 프로그램들은 같은 공유 쓰기전용 코드와 데이터를 사용하는 다수의 병령 실행 간 서로 공유됩니다. 즉 개발자들은 프로그램에 어떠한 고정 writable 및 글로벌 변수값을 넣으면 안된다는 것입니다. 미래에 writable data 지원을 위해 copy-on-write 메커니즘이 추가될 수 있습니다.

## Signed division

BPF 명령 세트는 [signed division](https://www.kernel.org/doc/html/latest/bpf/bpf_design_QA.html#q-why-there-is-no-bpf-sdiv-for-signed-divide-operation)을 지원하지 않습니다. Signed division 명령 추가는 고려중에 있습니다.

## Loaders

프로그램은 런타임 로더를 통해 배포되고 실행됩니다. 현재 [BPF 로더](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/bpf_loader.rs#L17)와 [비활성 BPF 로더](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/bpf_loader_deprecated.rs#L14) 두가지 로더가 지원되고 있습니다.

로더는 각자 다른 어플리케이션 바이너리 인터페이스를 지원할 수 있으니 개발자들은 프로그램 작성과 배포에 같은 로더를 써야합니다. 만일 프로그램이 한 로더로 쓰였고 다른 로더를 통해 배포되면 일반적으로 `AccessViolation` 에러가 발생합니다. 이는 프로그램 입력값 패리미터의 mismatched deserialization 때문입니다.

실용적인 목적으로 볼 때 프로그램은 항상 가장 최신 BPF 로더를 향해 작성되어야만 하고 커맨드라인 인터페이스와 자바스크립트 API에선 최신 로더가 디폴트로 설정되어 있습니다.

특정 로더를 위한 프로그램 적용에 대한 언어별 정보에 대해선 다음을 참고하세요:

- [러스트 프로그램 엔트리포인트](developing-rust.md#program-entrypoint)
- [C 프로그램 엔트리포인트](developing-c.md#program-entrypoint)

### 배포

BPF 프로그램 배포는 BPF 공유 객체를 프로그램 계정의 데이터로 업로드하며 계정을 실행가능하게 마킹하는 프로세스를 뜻합니다. 클라이언트는 BPF 공유 객체를 더 작은 조각으로 나누고 [`Write`](https://github.com/solana-labs/solana/blob/bc7133d7526a041d1aaee807b80922baa89b6f90/sdk/program/src/loader_instruction.rs#L13)의 명령 데이터로 보내며 로더는 해당 데이터를 프로그램의 계정 데이터로 쓰게 됩니다. 모든 조각을 받으면 클라이언트는 [Finalize</code>](https://github.com/solana-labs/solana/blob/bc7133d7526a041d1aaee807b80922baa89b6f90/sdk/program/src/loader_instruction.rs#L30)명령을 로더에 올리고, 로더는 BPF 데이터를 검증하고 프로그램 계정을 *executable*로 마크합니다. 프로그램 계정이 executable로 마크되면, 다음 트랜잭션들은 프로그램이 처리할 수 있는 명령을 발생시킬 수 있습니다.

executable BPF로 명령이 들어가면 로더는 프로그램의 실행 환경을 설정하고, 프로그램 입력값 패리미터를 순차화시키고, 프로그램의 엔트리포인트를 호출하고, 마주친 에러들을 보고합니다.

[배포](deploying.md)에서 더 자세히 알아보세요

### 입력 패리미터 직렬화

BPF 로더는 입력 패리미터를 온체인에서 비직렬화 시키는 프로그램 엔트리포인트로 넘길 수 있는 바이트 어레이로 직렬화 됩니다. 비활성화된 로더와 현재 로더 간 변경점 중 하나는 바로 입력 패리미터가 정렬된 바이트 어래이 상 정렬된 오프셋으로 다양한 패리미터들이 직렬화 될 수 있도록 해준다는 것입니다. 덕분에 바이트 어레이를 직접 참고할 수 있는 비직렬화 적용과 프로그램에게 정렬된 포인터를 제공할 수 있습니다.

특정 언어의 직렬화에 대해선 아래를 참고하세요:

- [러스트 프로그램 패리미터 비직렬화](developing-rust.md#parameter-deserialization)
- [C 프로그램 패리미터 비직렬화](developing-c.md#parameter-deserialization)

최신 로더는 다음과 같은 프로그램 입력 패리미터를 직렬화 합니다 (모든 인코딩은 endian 입니다):

- 8 바이트 서명되지 않은 계정 수
- 계정별
  - 중복계정 확인을 표시하는 1바이트. 만일 중복이 아니면 값은 0xff이며, 중복이면 중복인 계정의 인덱스가 값이 됩니다.
  - 패딩 7바이트
    - 중복이 아닐 시
      - 1바이트 패딩
      - 1바이트 불리안, 계정이 서명자일시 참
      - 1바이트 불리안, 계정이 쓰기가 되면 참
      - 1바이트 불리안, 계정이 실행 가능하면 참
      - 4바이트 패딩
      - 계정의 공개키 32바이트
      - 계정 소유자의 공개키 32바이트
      - 할당되지 않은 보유 계정의 램포트 수 8바이트
      - 서명되지 않은 계정 데이터 바이트 수 8바이트
      - 계정 데이터 x바이트
      - realloc에 쓰이는 10k 바이트 패딩
      - 오프셋이 8바이트가 될 수 있도록 충분한 패딩
      - 렌트 에폭 8바이트
- 서명되지 않은 명령 데이터 수 8바이트
- 명령 데이터 x바이트
- 프로그램 아이디 32바이트
