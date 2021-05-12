---
title: "런타임"
---

## 런타임

런타임은 소유자 프로그램이 계정을 인출하거나 데이터를 수정할 수만 허용합니다. 그런 다음 프로그램은 클라이언트가 소유 한 계정을 수정할 수 있는지 여부에 대한 추가 규칙을 정의합니다. 시스템 프로그램의 경우 사용자가 트랜잭션 서명을 인식하여 램프를 전송할 수 있습니다. 클라이언트가 키 쌍의 *private key*를 사용하여 트랜잭션에 서명 한 것을 확인하면 클라이언트가 토큰 전송을 승인 한 것입니다.

즉, 특정 프로그램이 소유 한 전체 계정 집합은 키가 계정 주소이고 값이 프로그램 별 임의의 이진 데이터 인 키-값 저장소로 간주 될 수 있습니다. 프로그램 작성자는 프로그램의 전체 상태를 가능한 많은 계정으로 관리하는 방법을 결정할 수 있습니다.

런타임은 각 트랜잭션의 지침을 실행 한 후 계정 메타 데이터를 사용하여 액세스 정책이 위반되지 않았는지 확인합니다. 프로그램이 정책을 위반하는 경우 런타임은 트랜잭션의 모든 명령에 의해 수행 된 모든 계정 변경 사항을 무시하고 트랜잭션을 실패한 것으로 표시합니다.

### 실행

엔진은 공개 키를 계정에 매핑하고이를 프로그램의 진입 점으로 라우팅합니다.

The policy is as follows:

- Only the owner of the account may change owner.
  - And only if the account is writable.
  - And only if the account is not executable
  - And only if the data is zero-initialized or empty.
- An account not assigned to the program cannot have its balance decrease.
- The balance of read-only and executable accounts may not change.
- Only the system program can change the size of the data and only if the system program owns the account.
- -소유자 만 계정 데이터를 변경할 수 있습니다.
  - And if the account is writable.
  - And if the account is not executable.
- Executable is one-way (false->true) and only the account owner may set it.
- No one modification to the rent_epoch associated with this account.

## 트랜잭션 엔진

프로그램이 계산 리소스를 남용하는 것을 방지하기 위해 트랜잭션의 각 명령에 계산 예산이 제공됩니다. 예산은 프로그램이 다양한 작업을 수행 할 때 사용되는 계산 단위와 프로그램이 초과 할 수없는 범위로 구성됩니다. 프로그램이 전체 예산을 소비하거나 한도를 초과하면 런타임이 프로그램을 중지하고 오류를 반환합니다.

다음 작업은 계산 비용을 발생 BPF 명령 실행 -시스템 호출 호출 -로깅 -프로그램 주소 생성 -교차 프로그램 호출 -...

- Executing BPF instructions
- Calling system calls
  - logging
  - creating program addresses
  - cross-program invocations
  - ...

시킵니다.-교차 프로그램 호출의 경우 호출 된 프로그램은 부모의 예산을 상속합니다. 호출 된 프로그램이 예산을 소비하거나 한도를 초과하면 전체 호출 체인과 상위가 중지됩니다.

런타임은 다음 규칙을 적용합니다.

프로그램의 실행에는 프로그램의 공개 키를 트랜잭션에 대한 포인터와로드 된 계정의 배열을 사용하는 진입 점에 매핑하는 것이 포함됩니다.

```rust
max_units : 200,000,
log_units : 100,
log_u64_units : 100,
create_program address units : 1500,
invoke_units : 1000,
max_invoke_depth : 4,
max_call_depth : 64,
stack_frame_size : 4096,
log_pubkey_units : 100 인 경우 ,
```

인터페이스는 사용자가 인코딩하는`Instruction :: data`로 가장 잘 설명됩니다.

- 그러면 프로그램이 -아무것도하지 않으면 200,000 BPF 명령을 실행할 수 있습니다.
- Could log 2,000 log messages
- Can not exceed 4k of stack usage
- Can not exceed a BPF call depth of 64
- Cannot exceed 4 levels of cross-program invocations.

프로그램이 실행됨에 따라 컴퓨팅 예산이 점진적으로 소비되므로 총 예산 소비는 수행하는 작업의 다양한 비용의 조합이됩니다.

런타임에 프로그램은 남아있는 컴퓨팅 예산의 양을 기록 할 수 있습니다. 자세한 내용은 \[debugging\] (developing / deployed-programs / debugging.md # monitoring-compute-budget-consumption)을 참조하십시오.

예산 값은 기능 사용을 조건으로합니다. 계산 예산의 \[new\] (https://github.com/solana-labs/solana/blob/d3a3a7548c857f26ec2cb10e270da72d373020ec/sdk/src/process_instruction.rs#L97) 함수를 살펴보십시오. 예산 구성 방법. 현재 예산 값을 결정하려면 \[features\] (runtime.md # features)의 작동 방식과 사용중인 클러스터에서 활성화 된 기능에 대한 이해가 필요합니다.

## 향후 작업

Solana가 발전함에 따라 클러스터의 동작과 프로그램 실행 방식을 변경하는 새로운 기능이나 패치가 도입 될 수 있습니다. 동작의 변경은 클러스터의 다양한 노드간에 조정되어야합니다. 노드가 조정되지 않으면 이러한 변경으로 인해 합의가 무너질 수 있습니다. Solana는 변경 사항을 원활하게 적용 할 수 있도록 런타임 기능이라는 메커니즘을 지원합니다.

런타임 기능은 클러스터에 대한 하나 이상의 동작 변경이 발생하는 에포크 조정 이벤트입니다. 동작을 변경할 Solana의 새로운 변경 사항은 기능 게이트로 래핑되고 기본적으로 비활성화됩니다. 그런 다음 Solana 도구를 사용하여 기능을 활성화하여 보류로 표시하고 보류로 표시하면 다음 에포크에서 기능이 활성화됩니다.

(CLI / install-solana-cli-tools.md) 사용하기 [솔라나 명령 줄 도구]를 활성화하는 기능을 확인하려면

```bash
solana feature status
```

발생하면 먼저 확인 그 솔라 도구 버전당신은`solana cluster-version`에서 반환 된 버전과 일치를 사용하고 있습니다. 일치하지 않는 경우 \[올바른 도구 모음 설치\] (cli / install-solana-cli-tools.md).
