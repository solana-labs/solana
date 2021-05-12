---
title: 유효성 검사기 모니터링
---

## 가십 확인

다음을 실행하여 유효성 검사기의 IP 주소와 ** ID pubkey **가 가십 네트워크에 표시되는지 확인합니다.

```bash
솔라나 가십 스파이 --entrypoint devnet.solana.com:8001
```

## 잔액 확인

귀하의 계정 잔액은 귀하의 밸리데이터이 투표를 제출할 때 거래 수수료 금액만큼 감소하고 리더 역할을 한 후에 증가해야합니다. `--lamports`를 전달하면 더 자세히 관찰 할 수 있습니다.

```bash
솔라나 밸런스-Lamports
```

## 투표 활동 확인

`solana vote-account` 명령은 유효성 검사기의 최근 투표 활동을 표시합니다.

```bash
솔라나 투표 계정 ~ / vote-account-keypair.json
```

## 클러스터 정보 가져 오기

클러스터의 유효성 검사기와 클러스터 상태를 모니터링하는 데 유용한 JSON-RPC 엔드 포인트가 몇 가지 있습니다.

```bash
# solana-gossip과 유사하게 클러스터 노드 목록에 밸리데이터가 표시되어야합니다. curl -X POST -H "Content-Type : application / json"-d '{ "jsonrpc": "2.0", "id": 1, "method": "getClusterNodes"}'http : //devnet.solana. com
# 밸리데이터이 올바르게 투표하면`현재`투표 계정 목록에 나타납니다. If staked, `stake` should be > 0
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getVoteAccounts"}' http://devnet.solana.com
# Returns the current leader schedule
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getLeaderSchedule"}' http://devnet.solana.com
# Returns info about the current epoch. slotIndex는 후속 호출에서 진행되어야합니다.
curl -X POST -H "Content-Type : application / json"-d '{ "jsonrpc": "2.0", "id": 1, "method": "getEpochInfo"}'http : //devnet.solana. com
```
