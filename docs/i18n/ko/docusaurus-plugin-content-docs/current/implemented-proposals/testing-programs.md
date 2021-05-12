---
title: 테스트 프로그램
---

애플리케이션은 트랜잭션을 Solana 클러스터로 보내고 유효성 검사기를 쿼리하여 트랜잭션이 처리되었는지 확인하고 각 트랜잭션의 결과를 확인합니다. 클러스터가 예상대로 작동하지 않는 경우 여러 가지 이유가있을 수 있습니다.

- The program is buggy
- -BPF 로더가 안전하지 않은 프로그램 명령을 거부했습니다.
- The transaction was too big
- The transaction was invalid
- -다른 트랜잭션이 액세스 중일 때 런타임이 트랜잭션을 실행하려고했습니다.

  the same account

- -네트워크가 거래를 중단했습니다.
- The cluster rolled back the ledger
- -밸리데이터이 악의적으로 질의에 응답

## AsyncClient 및 SyncClient 특성

문제를 해결하려면 응용 프로그램이 더 적은 오류가 발생할 수있는 하위 수준 구성 요소를 다시 대상으로 지정해야합니다. 대상 변경은 AsyncClient 및 SyncClient 특성의 다른 구현으로 수행 할 수 있습니다.

같은 계정

```text
trait AsyncClient {
    fn async_send_transaction (& self, transaction : Transaction)-> io :: Result <Signature>;
}

trait SyncClient {
    fn get_signature_status (& self, signature : & Signature)-> Result <Option <transaction :: Result <() >>>;
}
```

사용자는 트랜잭션을 전송하고 비동기식 및 동기식으로 결과를 기다립니다.

### 클러스터 용 ThinClient

가장 높은 수준의 구현 인 ThinClient는 배포 된 테스트 넷 또는 개발 머신에서 실행되는 로컬 클러스터 일 수있는 Solana 클러스터를 대상으로합니다.

### TPU 용 TpuClient

다음 수준은 아직 구현되지 않은 TPU 구현입니다. TPU 수준에서 애플리케이션은 Rust 채널을 통해 트랜잭션을 전송합니다. 네트워크 대기열이나 패킷 손실로 인한 놀라움이 없습니다. TPU는 모든 '정상적인'트랜잭션 오류를 구현합니다. 서명 확인을 수행하고, 사용중인 계정 오류를보고 할 수 있으며, 그렇지 않으면 기록 해시 증명과 함께 원장이 생성됩니다.

## 저수준 테스트

### 은행 용 BankClient

TPU 수준 아래에는 은행이 있습니다. 은행은 서명 확인을하거나 원장을 생성하지 않습니다. The Bank는 새로운 온 체인 프로그램을 테스트 할 수있는 편리한 계층입니다. 이를 통해 개발자는 기본 프로그램 구현과 BPF로 컴파일 된 변형간에 전환 할 수 있습니다. 여기서 Transact 특성이 필요하지 않습니다. 은행의 API는 동기식입니다.

## 런타임을 사용한 단위 테스트

뱅크 아래에는 런타임이 있습니다. 런타임은 단위 테스트를위한 이상적인 테스트 환경입니다. 런타임을 기본 프로그램 구현에 정적으로 링크함으로써 개발자는 가능한 가장 짧은 편집-컴파일-실행 루프를 얻습니다. 동적 연결이 없으면 스택 추적에 디버그 기호가 포함되며 프로그램 오류는 문제 해결이 간단합니다.
