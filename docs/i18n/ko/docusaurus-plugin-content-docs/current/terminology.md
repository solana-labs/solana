---
title: 용어
---

문서 전체에서 다음 용어가 사용됩니다.

## 계정

\[공개 키\] (terminology.md # public-key)로 주소가 지정되고 \[lamports\] (terminology.md # lamport)가 수명을 추적하는 영구 파일입니다.

## 앱

Solana 클러스터와 상호 작용하는 프런트 엔드 애플리케이션입니다.

## bank state

지정된 [ 틱 높이 ](terminology.md#tick-height)에서 원장의 모든 프로그램을 해석 한 결과입니다. 여기에는 0이 아닌 [ 네이티브 토큰 ](terminology.md#native-tokens)을 보유한 모든 [ 계정 ](terminology.md#account) 세트가 포함됩니다.

## block

[ 투표 ](terminology.md#ledger-vote)가 적용되는 원장의 연속적인 [ 항목 ](terminology.md#entry)입니다. [ 리더 ](terminology.md#leader)는 [ 슬롯 ](terminology.md#slot) 당 최대 1 개의 블록을 생성합니다.

## blockhash

주어진 <a href="terminology.md에서 [ 원장 ](terminology.md#ledger)의 사전 이미지 방지 [ 해시 ](terminology.md#hash) # block-height "> 블록 높이 </a>. 슬롯의 마지막 [ 항목 ID ](terminology.md#entry-id)에서 가져옴

## 블록 높이가져옴

현재 블록 아래의

 블록 </ 0> 수입니다. [ 생성 블록 ](terminology.md#genesis-block) 다음의 첫 번째 블록은 높이가 1입니다.</p> 



## 부트 스트랩 유효성 검사기

[ 검증 자 ](terminology.md#validator)로 [ 블록 ](terminology.md#block)을 생성합니다.



## CBC 블록

가장 작은 암호화 된 원장 청크, 암호화 된 원장 세그먼트는 많은 CBC 블록으로 구성됩니다. 정확히`ledger_segment_size / cbc_block_size`입니다.



## 클라이언트

[ 클러스터 ](terminology.md#cluster)를 활용하는 [ 노드 ](terminology.md#node).



## cluster

단일 [ 원장 ](terminology.md#ledger)을 유지하는 [ 검증 자 ](terminology.md#validator) 집합입니다.



## 확인 시간

[ 리더 ](terminology.md#leader)가 [ 틱 항목 ](terminology.md#tick)을 생성하고 [ 확인 된 차단 ](를 생성하는 사이의 wallclock 기간 terminology.md # confirmed-block).



## 확인 된 블록

A [블록](terminology.md#block) (terminology.md # block)은 [원장 투표](terminology.md#ledger-vote) (terminology.md # ledger-vote) 중 [supermajority](terminology.md#supermajority)  (terminology.md # supermajority)를 수신했으며 원장 해석은 지도자.



## 컨트롤 플레인

[ 클러스터 ](terminology.md#cluster)의 모든 [ 노드 ](terminology.md#node)를 연결하는 가십 네트워크.



## 쿨 다운 기간

[ 스테이크 ](terminology.md#stake) 이후 일부 [ 에포크 ](terminology.md#epoch)가 비활성화되었지만 점차적으로 인출 할 수있게되었습니다. 이 기간 동안 지분은 "비활성화"된 것으로 간주됩니다. 추가 정보 : [ 준비 및 쿨 다운 ](implemented-proposals/staking-rewards.md#stake-warmup-cooldown-withdrawal)



## 크레딧

[ 투표 크레딧 ](terminology.md#vote-credit)을 참조하세요.



## 데이터 플레인

[ 항목 ](terminology.md#entry)을 효율적으로 검증하고 합의를 얻는 데 사용되는 멀티 캐스트 네트워크입니다.



## drone

사용자의 개인 키에 대한 관리자 역할을하는 오프 체인 서비스입니다. 일반적으로 트랜잭션의 유효성을 검사하고 서명하는 역할을합니다.



## 항목

[ 원장 ](terminology.md#ledger)에있는 항목 ([ 틱 ](terminology.md#tick) 또는 [ 거래 항목 ](terminology.md) # transactions-entry).



## entry id

전 세계적으로 [ 항목 ](terminology.md#entry) 역할을하는 항목의 최종 콘텐츠에 대한 사전 이미지 방지 [ 해시 ](terminology.md#hash) 고유 식별자. 해시는 다음의 증거로 사용됩니다

- The entry being generated after a duration of time
- The specified [transactions](terminology.md#transaction) are those included in the entry
- The entry's position with respect to other entries in [ledger](terminology.md#ledger)

[ 역사 증명 ](terminology.md#proof-of-history)을 참조하세요.



## epoch

[ 리더 일정 ](terminology.md#leader-schedule)이 유효한 시간, 즉 [ 슬롯 ](terminology.md#slot) 수입니다.



## 수수료 계정

거래의 수수료 계정은 거래를 원장에 포함하는 비용을 지불하는 계정입니다. 이것은 거래의 첫 번째 계정입니다. 거래 비용을 지불하면 계정 잔액이 줄어들 기 때문에이 계정은 거래에서 읽기-쓰기 (쓰기 가능)로 선언되어야합니다.



## finality

When nodes representing 2/3rd of the [stake](terminology.md#stake) have a common [root](terminology.md#root).



## fork

일반적인 항목에서 파생되었지만 분기 된 [ 원장 ](terminology.md#ledger)입니다.



## 제네시스 블록

체인의 첫 번째 [ 차단 ](terminology.md#block)입니다.



## genesis config

[ 생성 블록 ](terminology.md#genesis-block)을 위해 [ 원장 ](terminology.md#ledger)을 준비하는 구성 파일입니다.



## hash

바이트 시퀀스의 디지털 지문입니다.



## 인플레이션

바이트 시퀀스의 디지털 지문입니다.



## 명령

[ 클라이언트 ](terminology.md#client)가 <a href="에 포함할 수있는 [ 프로그램 ](terminology.md#program)의 가장 작은 단위입니다. terminology.md # transaction "> 거래 </a>.



## 키 쌍

[ 공개 키 ](terminology.md#public-key) 및 해당 [ 비공개 키 ](terminology.md#private-key).



## lamport

0.000000001 [ sol ](terminology.md#sol) 값을 갖는 분수 [ 네이티브 토큰 ](terminology.md#native-token)입니다.



## 리더

[ 검사기 ](terminology.md#validator)가 [ 항목 ](terminology.md#entry)을 [ 원장 ](terminology에 추가 할 때의 역할 .md # ledger).



## 리더 스케줄

일련의 [ 유효성 검사기 ](terminology.md#validator) [ 공개 키 ](terminology.md#public-key). 클러스터는 리더 일정을 사용하여 어떤 검증자가 [ 리더 ](terminology.md#leader)인지 언제든지 결정합니다.



## 원장

<a href="terminology.md#client에서 서명한 [ 거래 ](terminology.md#transaction)가 포함 된 [ 항목 ](terminology.md#entry) 목록 "> 고객 </a>. 개념적으로 이것은 [ 생성 블록 ](terminology.md#genesis-block)으로 거슬러 올라갈 수 있지만 실제 [ 검증 자 ](terminology.md#validator) ' 원장은 설계에 의한 향후 블록 검증에 필요하지 않은 이전 블록으로 스토리지 사용량을 저장하기 위해 최신 [ 블록 ](terminology.md#block) 만 가질 수 있습니다.



## 원장 투표

주어진 <a href="terminology에서 [ 검증 자 상태 ](terminology.md#bank-state)의 [ 해시 ](terminology.md#hash)입니다. md # tick-height "> 틱 높이 </a>. 수신 된 [ 차단 ](terminology.md#block)이 확인되었다는 [ 검증 자 ](terminology.md#validator)의 확인과 특정 항목에 대해 충돌하는 [ 차단 ](terminology.md#block) \ (예 : [ 포크 ](terminology.md#fork) \)에 투표하지 않겠다는 약속 시간, [ 잠금 ](terminology.md#lockout) 기간.



## light client

유효한 [ 클러스터 ](terminology.md#cluster)를 가리키고 있는지 확인할 수있는 [ 클라이언트 ](terminology.md#client) 유형입니다. [ 씬 클라이언트 ](terminology.md#thin-client)보다 더 많은 원장 확인을 수행하고 [ 유효성 검사기 ](terminology.md#validator)보다 적게 수행합니다.



## 로더

다른 온 체인 프로그램의 바이너리 인코딩을 해석하는 기능이있는 [ 프로그램 ](terminology.md#program).



## lockout

[ 검증 자 ](terminology.md#validator)가 다른 <a에 [ 투표 ](terminology.md # ledger-vote) 할 수없는 기간 href = "terminology.md # fork"> 포크 </a>.



## native token

클러스터의 [ 노드 ](terminology.md#node)에서 수행 한 작업을 추적하는 데 사용되는 [ 토큰 ](terminology.md#token)입니다.



## node

[ 클러스터 ](terminology.md#cluster)에 참여하는 컴퓨터.



## node count

[ 클러스터 ](terminology.md#cluster)에 참여하는 [ 검증 자 ](terminology.md#validator)의 수입니다.



## 역사증명

[ 역사 증명 ](terminology.md#proof-of-history)을 참조하세요.



## 포인트

보상 제도의 가중 [ 크레딧 ](terminology.md#credit)입니다. [ 검증 자 ](terminology.md#validator) [ 보상 제도 ](cluster/stake-delegation-and-rewards.md)에서 지급해야하는 포인트 수 사용 중 [ 스테이크 ](terminology.md#stake)는 획득 한 [ 투표 크레딧 ](terminology.md#vote-credit)과 등불이 걸렸다.



## 개인 키

[ 키 쌍 ](terminology.md#keypair)의 개인 키입니다.



## program

[ 안내 ](terminology.md#instruction)를 해석하는 코드입니다.



## 프로그램 ID

[ 프로그램 ](terminology.md#program)이 포함 된 [ 계정 ](terminology.md#account)의 공개 키입니다.



## 역사

증명 증명이 생성되기 전에 일부 데이터가 존재했으며 이전 증명 이전에 정확한 시간이 경과했음을 증명하는 증명 스택입니다. [ VDF ](terminology.md#verifiable-delay-function)와 마찬가지로 기록 증명은 생산하는 데 걸리는 시간보다 짧은 시간에 검증 할 수 있습니다.



## 공개 키

[ 키 쌍 ](terminology.md#keypair)의 공개 키입니다.



## root

최대 <a href="terminology.md#lockout에 도달한 [ 블록 ](terminology.md#block) 또는 [ 슬롯 ](terminology.md#slot) [ 검사기 ](terminology.md#validator)에서 "> 잠금 </a>. 루트는 유효성 검사기에서 모든 활성 포크의 조상 인 가장 높은 블록입니다. 루트의 모든 조상 블록도 전 이적으로 루트입니다. 조상이 아니고 루트의 후손이 아닌 블록은 합의 고려 대상에서 제외되며 버릴 수 있습니다.



## runtime

[ 프로그램 ](terminology.md#program) 실행을 담당하는 [ 검증기 ](terminology.md#validator)의 구성 요소입니다.



## 파쇄

[ 차단 ](terminology.md#block)의 일부; [ 검증 자 ](terminology.md#validator)간에 전송되는 가장 작은 단위입니다.



## 서명

R (32 바이트) 및 S (32 바이트)의 64 바이트 ed25519 서명입니다. R은 작은 순서가 아닌 패킹 된 Edwards 점이고 S는 0 <= S <L 범위의 스칼라 여야한다는 요구 사항이 있습니다.이 요구 사항은 서명 가단성을 보장하지 않습니다. 각 거래에는 [ 수수료 계정 ](terminology#fee-account)에 대한 서명이 하나 이상 있어야합니다. 따라서 트랜잭션의 첫 번째 서명은 [ 트랜잭션 ID ](terminology.md#transaction-id)로 처리 될 수 있습니다.



## slot

The period of time for which a [leader](terminology.md#leader) ingests transactions and produces a [block](terminology.md#block).



## 스마트 컨트랙트

일단 충족되면 미리 정의 된 계정 업데이트가 허용됨을 프로그램에 알리는 일련의 제약 조건입니다.



## sol

Solana 회사에서 인정하는 [ 클러스터 ](terminology.md#cluster)에서 추적하는 [ 네이티브 토큰 ](terminology.md#native-token)입니다.



## 지분

악의적 인 [ 검증 자 ](terminology.md#validator) 동작이 입증 될 수있는 경우 토큰은 [ 클러스터 ](terminology.md#cluster)에 몰수됩니다.



## supermajority

[ 클러스터 ](terminology.md#cluster)의 2/3.



## sysvar

프로그램이 현재 틱 높이, 보상 [ 포인트와 같은 네트워크 상태에 액세스 할 수 있도록 런타임에서 제공하는 합성 [ 계정 ](terminology.md#account) ](terminology.md#point) 값 등



## thin client

유효한 [ 클러스터 ](terminology.md#cluster)와 통신하고 있음을 신뢰하는 [ 클라이언트 ](terminology.md#client) 유형입니다.



## tick

벽시계 기간을 추정하는 원장 [ 항목 ](terminology.md#entry)입니다.



## tick height

[ 원장 ](terminology.md#ledger)의 N 번째 [ 틱 ](terminology.md#tick)입니다.



## 토큰 토큰

토큰 세트의 희귀하고 대체 가능한 구성원입니다.



## tps

초당 [ 거래 ](terminology.md#transaction).



## transaction

하나 이상의 <a href="terminology를 사용하여 [ 클라이언트 ](terminology.md#client)가 서명 한 하나 이상의 [ 안내 ](terminology.md#instruction) .md # keypair "> 키쌍 </a>을 생성하고 성공 또는 실패라는 두 가지 가능한 결과 만 원자 적으로 실행합니다.



## transaction id

[ 트랜잭션 ](terminology.md#transaction)의 첫 번째 [ 서명 ](terminology.md#signature)으로, 전체에서 트랜잭션을 고유하게 식별하는 데 사용할 수 있습니다. [ 원장 ](terminology.md#ledger)을 작성합니다.



## transaction confirmations

[ 원장 ](terminology.md#ledger)에서 거래가 수락 된 이후 [ 확인 된 차단 ](terminology.md#confirmed-block) 수입니다. 블록이 [ 루트 ](terminology.md#root)가되면 거래가 완료됩니다.



## 트랜잭션 항목

병렬로 실행될 수있는 [ 트랜잭션 ](terminology.md#transaction) 세트입니다.



## validator

[ 원장 ](terminology.md#ledger)의 유효성을 검사하고 새 <a href=를 생성하는 작업을 담당하는 [ 클러스터 ](terminology.md#cluster)의 전체 참여자 "terminology.md # block"> 차단 </a>.



## VDF

[ 확인 가능한 지연 함수 ](terminology.md#verifiable-delay-function)를 참조하세요.



## 검증 가능한 지연 기능

실행에 고정 된 시간이 걸리는 함수로, 실행 된 증명을 생성하고 생성하는 데 걸리는 시간보다 짧은 시간에 확인할 수 있습니다.



## vote

[ 원장 투표 ](terminology.md#ledger-vote)를 참조하세요.



## 투표 크레딧

[ 검증 자 ](terminology.md#validator)에 대한 보상 집계입니다. 검증자가 [ 루트 ](terminology.md#root)에 도달하면 투표 계정의 검증 자에게 투표 크레딧이 부여됩니다.



## wallet

[ 키 쌍 ](terminology.md#keypair) 모음입니다.



## 워밍업 기간

[ 스테이크 ](terminology.md#stake) 후 일부 [ 에포크 ](terminology.md#epoch)가 위임되었지만 점진적으로 효력이 발생합니다. 이 기간 동안 스테이킹는 "활성화"된 것으로 간주됩니다. 추가 정보 : [ 준비 및 쿨 다운 ](cluster/stake-delegation-and-rewards.md#stake-warmup-cooldown-withdrawal)
