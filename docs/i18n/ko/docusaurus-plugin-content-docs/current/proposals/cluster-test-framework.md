---
title: 클러스터 테스트 프레임 워크
---

이 문서는 클러스터 테스트 프레임 워크 \ (CTF \)를 제안합니다. CTF는 로컬, 프로세스 내 클러스터 또는 배포 된 클러스터에 대해 테스트를 실행할 수있는 테스트 도구입니다.

## 동기

CTF의 목표는 클러스터가 배포되는 위치와 방법에 관계없이 테스트를 작성하기위한 프레임 워크를 제공하는 것입니다. 이러한 테스트에서 회귀를 캡처 할 수 있으며 배포를 확인하기 위해 배포 된 클러스터에 대해 테스트를 실행할 수 있습니다. 이러한 테스트의 초점은 클러스터 안정성, 합의, 내결함성, API 안정성에 있어야합니다.

테스트는 단일 버그 또는 시나리오를 확인해야하며 테스트에 노출 된 내부 배관의 양을 최소화하여 작성해야합니다.

## 디자인 개요

테스트에는`contact_info :: ContactInfo` 구조 인 진입 점과 이미 자금이 지원 된 키 쌍이 제공됩니다.

클러스터의 각 노드는 부팅시`validator :: ValidatorConfig`로 구성됩니다. 부팅시이 구성은 테스트에 필요한 추가 클러스터 구성을 지정합니다. 클러스터는 in-process 또는 데이터 센터에서 실행될 때 구성으로 부팅되어야합니다.

일단 부팅되면 테스트는 가십 진입 점을 통해 클러스터를 검색하고 유효성 검사기 RPC를 통해 런타임 동작을 구성합니다.

## 테스트 인터페이스

각 CTF 테스트는 불투명 한 진입 점과 펀딩 된 키 쌍으로 시작됩니다. 테스트는 클러스터 배포 방법에 의존해서는 안되며 공개적으로 사용 가능한 인터페이스를 통해 모든 클러스터 기능을 실행할 수 있어야합니다.

```text
crate :: contact_info :: ContactInfo 사용;
solana_sdk :: signature :: {Keypair, Signer}를 사용합니다. pub fn test_this_behavior (
    entry_point_info : & ContactInfo,
    fund_keypair : & Keypair,
    num_nodes : 사용,
)
```

## 클러스터 검색

테스트 시작시 클러스터가 이미 설정되었으며 완전히 연결되었습니다. 테스트는 몇 초 동안 사용 가능한 대부분의 노드를 검색 할 수 있습니다.

```text
crate :: gossip_service :: discover_nodes를 사용하십시오. // 몇 초에 걸쳐 클러스터를 검색합니다.
let cluster_nodes = discover_nodes (& entry_point_info, num_nodes);
```

## 클러스터 구성

특정 시나리오를 사용하려면 클러스터를 특수 구성으로 부팅해야합니다. 이러한 구성은`validator :: ValidatorConfig`에서 캡처 할 수 있습니다.

예를 들면 다음과 같습니다.

```text
let mut validator_config = ValidatorConfig :: default ();
validator_config.rpc_config.enable_validator_exit = true;
let local = LocalCluster :: new_with_config (
                num_nodes,
                10_000,
                100,
                & validator_config
                );
```

## 새 테스트를 디자인하는 방법

예를 들어 잘못된 광고 가십 노드로 가득 차면 클러스터가 실패 함을 보여주는 버그가 있습니다. 가십 라이브러리 및 프로토콜이 변경 될 수 있지만 클러스터는 여전히 잘못된 광고 가십 노드의 홍수에 대해 탄력성을 유지해야합니다.

RPC 서비스를 구성합니다.

```text
let mut validator_config = ValidatorConfig :: default ();
validator_config.rpc_config.enable_rpc_gossip_push = true;
validator_config.rpc_config.enable_rpc_gossip_refresh_active_set = true;
```

RPC를 연결하고 새 테스트를 작성합니다.

```text
pub fn test_large_invalid_gossip_nodes (
    entry_point_info : & ContactInfo,
    fund_keypair : & Keypair,
    num_nodes : 사용,
) {
    let cluster = discover_nodes (& entry_point_info, num_nodes);

    // 클러스터를 독살합니다.
    let client = create_client (entry_point_info.client_facing_addr (), VALIDATOR_PORT_RANGE);
    for _ in 0 .. (num_nodes * 100) {
        client.gossip_push (
            cluster_info :: invalid_contact_info ()
        );
    }
    sleep (지속 시간 :: from_millis (1000));

    // 활성 세트를 강제로 새로 고칩니다.
    for node in &cluster {
        let client = create_client(node.client_facing_addr(), VALIDATOR_PORT_RANGE);
        client.gossip_refresh_active_set();
    }

    // Verify that spends still work.
    verify_spends (& 클러스터);
}
```
