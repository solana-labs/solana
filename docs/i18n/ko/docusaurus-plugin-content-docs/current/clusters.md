---
title: Solana Clusters
---

Solana는 다른 목적으로 여러 다른 클러스터를 유지합니다.

시작하기 전에 먼저 \[Solana 명령 줄 도구를 설치했는지\] (cli / install-solana-cli-tools.md)](https

Explorers ::explorer

- Devnet 용 Gossip 진입 점 :`entrypoint.devnet.solana.com : 8001`Devnet 용 -측정 항목 환경 변수 :
- [http://solanabeach.io/](http://solanabeach.io/).

## Devnet

- Devnet serves as a playground for anyone who wants to take Solana for a test drive, as a user, token holder, app developer, or validator.
- -애플리케이션 개발자는 Devnet을 대상으로해야합니다.
- -잠재적 밸리데이터는 먼저 Devnet을 대상으로해야합니다.
- Key differences between Devnet and Mainnet Beta:
  - Devnet tokens are **not real**
  - Devnet includes a token faucet for airdrops for application testing
  - Devnet may be subject to ledger resets
  - -의 DevNet 사이 키 차이 Mainnet 베타 -의DevNet 토큰은 **하지 실제 ** -의 DevNet 응용 프로그램 테스트를위한 airdrops에 대한 토큰 수도꼭지를 포함 -의 DevNet이 원장 재설정 대상이 될 수 있습니다 -의 DevNet는 일반적으로 Mainnet 베타보다 최신 소프트웨어 버전을 실행
- Gossip entrypoint for Devnet: `entrypoint.devnet.solana.com:8001`
- Metrics environment variable for Devnet:
```bash
export SOLANA_METRICS_CONFIG = "host = https : //metrics.solana.com : 8086, db = devnet, u = scratch_writer, p = topsecret "
```
- -Devnet 용 RPC URL :`https : // devnet.solana.com`

##### Example `solana` command-line configuration

```bash
구성<code>bash는
솔라 설정 세트 --url
https://api.mainnet-beta.solana.com</code>
```

##### '예 solana` 명령 라인

```bash
export SOLANA_METRICS_CONFIG = "host = https : //metrics.solana.com : 8086, db = mainnet-beta, u = mainnet-beta_write, p = password "
```

예시`solana` 명령 줄

## Mainnet

- Testnet is where we stress test recent release features on a live cluster, particularly focused on network performance, stability and validator behavior.
- [Tour de SOL](tour-de-sol.md) initiative runs on Testnet, where we encourage malicious behavior and attacks on the network to help us find and squash bugs or network vulnerabilities.
- Testnet tokens are **not real**
- Testnet may be subject to ledger resets.
- Testnet includes a token faucet for airdrops for application testing
- Testnet typically runs a newer software release than both Devnet and Mainnet Beta
- Gossip entrypoint for Testnet: `entrypoint.testnet.solana.com:8001`
- Metrics environment variable for Testnet:
```bash
export SOLANA_METRICS_CONFIG="host=https://metrics.solana.com:8086,db=tds,u=testnet_write,p=c4fa841aa918bf8274e3e2a44d77568d9861b3ea"
```
- -메인 넷 베타 용 RPC URL :`https : // api.mainnet-beta.solana.com`

##### Example `solana` command-line configuration

```bash
구성``떠들썩한파티
솔라 구성 세트 --url
https://testnet.solana.com```#####
```

##### 예`솔라나 - validator` 명령

```bash
줄<code>bash는
$ 솔라 -validator \
    --identity ~ / 검증 - keypair.json \
    --vote-계정 ~ / 투표-계정 keypair.json \
    --trusted - 검증 7Np41oeYqPefeNQEHSv1UDhYrehxin3NStELsSKCT4K2 \
    --trusted - 검증 GdnSyH3YtwcxFvQrVVJMm1JhTS4QVX7MFsX56uJLUfiZ \
    --trusted - 검증의 DE1bawNcRJB9rVm3buyMVfr8mBEoyyu73NBovf2oXJsJ \
    --trusted-validator CakcnaRDHka2gXyfbEd2d3xsvkJkqsLw2akB3zsN1D2S \
    --no-untrusted-rpc \
    --ledger ~ / validator-ledger \
    --rpc-port 8899 \
    --private-rpc \
    --dynamic-port-range 8000-8010 \
    - 진입 점 t entrypoint.mainnet-beta.solana.com:8001 \
    --entrypoint entrypoint2.mainnet-beta.solana.com:8001 \
    --entrypoint entrypoint3.mainnet-beta.solana.com:8001 \
    --entrypoint entrypoint4.mainnet- beta.solana.com:8001 \
    --entrypoint entrypoint5.mainnet-beta.solana.com:8001 \
    --expected - 창세기 해시 5eykt4UsFv8P8NJdTREpY1vzqKqZKvdpKuc147dw2N9d \
    --wal 복구 모드 skip_any_corrupted_record \
    --limit 원장
크기의</code>
```

4 개의`--trusted-validator`는 모두 Solana에서 운영합니다.

- `5D1fNXzvv5NjV1ysLjirC4WY92RNsVH18vjmcszZd8on` - testnet.solana .COM (솔라) -`ta1Uvfb7W5BRPrdGnhP9RmeCGKzBySGM1hTE4rBRy6T` - 브레이크 RPC 노드 (솔라) -`Ft5fbkqNa76vnsjYNwjDZUXoTWpP7VYm3mtsaQckQADN` - CERTUS 하나 -`9QxCLckBiJc783jnMvXZubK4wH86Eqqvashtrwvcsgkv` - 너 한테 | 스테이킹
- `ta1Uvfb7W5BRPrdGnhP9RmeCGKzBySGM1hTE4rBRy6T` - Break RPC node (Solana)
- `Ft5fbkqNa76vnsjYNwjDZUXoTWpP7VYm3mtsaQckQADN` - Certus One
- `9QxCLckBiJc783jnMvXZubK4wH86Eqqvashtrwvcsgkv` - Algo|Stake

## Mainnet Beta

베타의permissionless, 초기 토큰 소지자 및 발사 파트너를위한 지속적인 클러스터. 현재 보상과 인플레이션은 비활성화되어 있습니다.

- Tokens that are issued on Mainnet Beta are **real** SOL
- -메인 넷 베타에서 발행되는 토큰은 ** 진짜 ** SOL입니다 .-코인리스트 경매 등을 통해 토큰을 구매 / 발행하기 위해 돈을 지불 한 경우, 이러한 토큰은 메인 넷 베타로 전송됩니다.
  - -참고 : \[Solflare\] (wallet-guide / solflare.md)와 같은 비 명령 줄 지갑을 사용하는 경우 지갑은 항상 메인 넷 베타에 연결됩니다.
- Gossip entrypoint for Mainnet Beta: `entrypoint.mainnet-beta.solana.com:8001`
- -메인 넷 베타 용 가십 진입 점 :`entrypoint.mainnet-beta.solana.com : 8001`-메인 넷 베타 용 측정 항목 환경 변수 :
```bash
export SOLANA_METRICS_CONFIG="host=https://metrics.solana.com:8086,db=mainnet-beta,u=mainnet-beta_write,p=password"
```
- RPC URL for Mainnet Beta: `https://api.mainnet-beta.solana.com`

##### Example `solana` command-line configuration

```bash
solana config set --url https://api.mainnet-beta.solana.com
```

##### 실시 예 솔라-validator` 명령 -

```bash
라인<code>bash는
$의 솔라 - 검증 \
    --identity ~ / 검증 - keypair.json \
    --vote-계정 ~ / 투표-계정 keypair.json \
    --trusted - 검증 5D1fNXzvv5NjV1ysLjirC4WY92RNsVH18vjmcszZd8on \
    --trusted - 검증의 ta1Uvfb7W5BRPrdGnhP9RmeCGKzBySGM1hTE4rBRy6T \
    --trusted-validator Ft5fbkqNa76vnsjYNwjDZUXoTWpP7VYm3mtsaQckQADN \
    --trusted-validator 9QxCLckBiJc783jnMvXZubK4wH86Eqqvashtrwvcsg kv \
    --no-untrusted-rpc \
    --ledger ~ / validator-ledger \
    --rpc-port 8899 \
    --dynamic-port-range 8000-8010 \
    --entrypoint entrypoint.testnet.solana.com:8001 \
    --expected - 기원 - 해시 4uhcVJyU9pJkvQyS88uRDiswHXSCkY3zQawwpjk2NsNY \
    --wal 복구 모드 skip_any_corrupted_record \
    --limit 원장
크기</code>은`--trusted
```

validator`s의 ID는 다음과 같습니다 :
