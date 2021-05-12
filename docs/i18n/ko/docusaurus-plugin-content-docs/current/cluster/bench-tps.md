---
title: 클러스터 벤치마킹
---

솔라나 git repository에는 자체 로컬 테스트넷을 가동하는 데 필요한 모든 스크립트가 포함되어 있습니다. 달성하고자하는 것에 따라, 멀티 노드 테스트넷은 Rust 전용 단일 테스트 노드보다 설정하기가 훨씬 더 복잡하기 때문에 다른 유형의 노드를 실행하는게 좋을 수 있습니다. 스마트 컨트렉트 실험과 같은 기능을 개발하려는 경우 설정 이슈를 줄이기 위해 Rust 전용 단일 노드 데모를 사용해주세요. 트랜잭션 파이프라인의 성능 최적화를 작업하는 경우 단일 노드 데모를 추천합니다. 컨센선스 관련 작업을하고 있다면 Rust 전용 멀티 노드 데모가 필요합니다. TPS 메트릭을 작업하거나 재현하려면 다중 노드 데모를 실행해주세요.

네 가지 종류 모두 최신 Rust toolchain과 솔라나 소스 코드가 필요합니다.

먼저 Solana [ README](https://github.com/solana-labs/solana#1-install-rustc-cargo-and-rustfmt)에 설명 된대로 Rust, Cargo 및 시스템 패키지를 설정합니다.

이제 github에서 코드를 확인합니다.

```bash
git clone https://github.com/solana-labs/solana.git
cd solana
```

새로운 기능을 추가 할 때 데모 코드가 릴리스간에 크러시가 나기도 하므로 데모를 처음 실행하는경우를 [최신 릴리스](https://github.com/solana-labs/solana/releases)를 확인하면 성공 확률이 높아질 것입니다.

```bash
TAG=$(git describe --tags $(git rev-list --tags --max-count=1))
git checkout $TAG
```

### 구성 설정

노드가 시작되기 전에 투표 프로그램과 같은 중요한 프로그램이 빌드되었는지 확인하세요. 예시로는 좋은 성능을 위해 릴리스 빌드를 사용하고 있다는 사실을 인지하시기 바랍니다. 디버그 빌드를 원하면 `cargo build` 만 사용하고 명령어의 `NDEBUG = 1` 부분을 생략합니다.

```bash
cargo build --release
```

네트워크는 다음 스크립트를 실행하여 생성된 제네시스 원장으로 초기화됩니다.

```bash
NDEBUG=1 ./multinode-demo/setup.sh
```

### Faucet

밸리데이터와 클라이언트가 작동하려면 몇가지 테스트 토큰을 위해 faucet를 실행해야합니다. faucet은 테스트 트랜잭션에 사용할 수 있는 Milton Friedman 스타일의 "에어 드랍"\ (요청 클라이언트에게 무료 토큰 \)을 제공합니다.

다음 명령어로 faucet를 시작하세요.

```bash
NDEBUG=1 ./multinode-demo/faucet.sh
```

### 단일 노드 테스트넷

밸리데이터를 시작하기 전에 데모의 부트스트랩 밸리데이터를 사용할 컴퓨터의 IP 주소를 알고 있는지 확인하고 감시하려는 모든 컴퓨터에서 udp 포트 8000-10000이 열려 있는지 확인하세요.

이제 별도의 셸에서 부트스트랩 유효성 검사를 시작합니다.

```bash
NDEBUG=1 ./multinode-demo/bootstrap-validator.sh
```

서버가 초기화 될 때까지 몇 초 정도 기다리십시오. 거래를받을 준비가되면 "leader ready ..."가 출력됩니다. 만일 리더가 토큰이 없다면 faucet에 토큰을 요청할 것입니다. 리더 스타트를 위해 faucet를 작동 할 필요는 없습니다.

### 멀티 노드 테스트넷

다중 노드 테스트넷을 실행하려면 리더 노드를 시작한 후 별도의 셸에서 몇 가지 추가 밸리데이터를 가동합니다.

```bash
NDEBUG=1 ./multinode-demo/validator-x.sh
```

Linux에서 성능이 향상된 밸리데이터를 실행하려면 [ CUDA 10.0 ](https://developer.nvidia.com/cuda-downloads)시스템에 설치되어 있어야합니다.

```bash
./fetch-perf-libs.sh
NDEBUG=1 SOLANA_CUDA=1 ./multinode-demo/bootstrap-validator.sh
NDEBUG=1 SOLANA_CUDA=1 ./multinode-demo/validator.sh
```

### 테스트넷 클라이언트 데모

이제 단일 노드 또는 다중 노드 테스트넷이 실행중 이므로 트랜잭션을 보내 봅시다!

별도의 셸에서 클라이언트를 시작합니다.

```bash
NDEBUG=1 ./multinode-demo/bench-tps.sh # 기본적으로 localhost에서 실행됩니다.
```

현재 클라이언트 데모는 여러 스레드를 회전시켜 가능한 한 빨리 테스트넷에 500,000 개의 트랜잭션을 전송합니다. 그런 다음 클라이언트는 테스트넷을 주기적으로 핑하여 해당 시간에 처리한 트랜잭션 수를 확인합니다. 데모는 의도적으로 UDP 패킷으로 네트워크를 플러딩하므로 네트워크가 거의 확실하게 패킷을 삭제합니다. 이를 통해 테스트 넷은 71만 TPS에 도달 할 수 있습니다. 클라이언트 데모는 테스트넷이 추가 트랜잭션을 처리하지 않을 것이라고 확실하게 된 후에 완료됩니다. 화면에 여러 TPS 측정이 출력된 것을 볼 수 있습니다. 다중 노드 변형에서 각 밸리데이터 노드에 대한 TPS 측정도 볼 수 있습니다.

### 테스트넷 디버깅

코드에는 몇 가지 유용한 디버그 메시지가 있으며 모듈 및 수준별로 활성화 할 수 있습니다. 리더 또는 밸리데이터를 실행하기 전에 일반 RUST_LOG 환경 변수를 설정하십시오.

예제

- 모든 곳에서 `info`를 활성화하고 solana :: banking_stage 모듈에서만 `debug`를 활성화하려면 :

  ```bash
  export RUST_LOG=solana=info,solana::banking_stage=debug
  ```

- BPF 프로그램 로깅을 활성화하려면 :

  ```bash
  export RUST_LOG=solana_bpf_loader=trace
  ```

일반적으로 자주 사용하지 않는 디버그 메시지에는 `debug`를 사용하고, 자주 발생하는 메시지에는 `trace`를 사용하고 성능 관련 로깅에는 `info`를 사용합니다.

GDB를 사용하여 실행중인 프로세스에 연결할 수도 있습니다. 리더의 프로세스 이름은 *solana-validator*입니다.

```bash
sudo gdb
attach <PID>
set logging on
thread apply all bt
```

그러면 모든 스레드 스택 추적이 gdb.txt로 덤프됩니다.

## 개발자 테스트넷

이 예시에서 클라이언트는 공개 테스트넷에 연결합니다. 테스트넷에서 밸리데이터를 실행하려면 udp 포트 `8000-10000`을 열어야합니다.

```bash
NDEBUG=1 ./multinode-demo/bench-tps.sh --entrypoint devnet.solana.com:8001 --faucet devnet.solana.com:9900 --duration 60 --tx_count 50
```

[ 메트릭 대시 보드 ](https://metrics.solana.com:3000/d/monitor/cluster-telemetry?var-testnet=devnet)에서 클라이언트의 트랜잭션 효과를 관찰 할 수 있습니다.
