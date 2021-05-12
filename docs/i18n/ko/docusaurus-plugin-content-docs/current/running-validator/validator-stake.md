---
title: Staking
---

**By default your validator will have no stake.** This means it will be ineligible to become leader.

## 모니터링 따라 잡기

지분을 위임하려면 먼저 유효성 검사기가 실행 중이고 클러스터를 따라 잡았는지 확인하십시오. 유효성 검사기가 부팅 된 후 따라 잡는 데 약간의 시간이 걸릴 수 있습니다. 이 과정을 통해 검증을 모니터링하는`catchup` 명령을

```bash
비활성화해야합니다<code>bash는
솔라비활성화-지분 ~ / 검증 -
지분-keypair.json</code>와
```

와

또한 클러스터의 슬롯이 자신의 슬롯보다 빠르게 진행되는 경우 결코 따라 잡을 수 없을 것입니다. 이것은 일반적으로 유효성 검사기와 나머지 클러스터 사이의 일종의 네트워킹 문제를 의미합니다.

## 스테이킹

키 페어 생성 아직하지 않았다면 스테이킹 키 페어를 생성하십시오. 이 단계를 완료하면 Solana 런타임 디렉토리에 "validator-stake-keypair.json"이 표시됩니다.

```bash
solana-keygen new -o ~/validator-stake-keypair.json
```

## 지분을

Now delegate 1 SOL to your validator by first creating your stake account:

```bash
solana create-stake-account ~/validator-stake-keypair.json 1
```

유용한 JSON-RPC 메소드 :

```bash
solana delegate-stake ~/validator-stake-keypair.json ~/vote-account-keypair.json
```

> Don’t delegate your remaining SOL, as your validator will use those tokens to vote.

유용한 JSON-RPC 메소드 :

```bash
solana delegate-stake ~/validator-stake-keypair.json ~/some-other-vote-account-keypair.json
```

Assuming the node is voting, now you're up and running and generating validator rewards. Rewards are paid automatically on epoch boundaries.

The rewards lamports earned are split between your stake account and the vote account according to the commission rate set in the vote account. Rewards can only be earned while the validator is up and running. Further, once staked, the validator becomes an important part of the network. In order to safely remove a validator from the network, first deactivate its stake.

At the end of each slot, a validator is expected to send a vote transaction. These vote transactions are paid for by lamports from a validator's identity account.

This is a normal transaction so the standard transaction fee will apply. The transaction fee range is defined by the genesis block. The actual fee will fluctuate based on transaction load. You can determine the current fee via the [RPC API “getRecentBlockhash”](developing/clients/jsonrpc-api.md#getrecentblockhash) before submitting a transaction.

Learn more about [transaction fees here](../implemented-proposals/transaction-fees.md).

## Validator Stake Warm-up

To combat various attacks on consensus, new stake delegations are subject to a [warm-up](/staking/stake-accounts#delegation-warmup-and-cooldown) period.

Monitor a validator's stake during warmup by:

- 워밍업 중에 검증지분을 모니터링합니다 인의.-투표 계정보기 :`solana vote-account ~ / vote-account-keypair.json` 이것은 밸리데이터이 네트워크에 제출 한 모든 투표의 현재 상태를 표시합니다.
- View your stake account, the delegation preference and details of your stake:`solana stake-account ~/validator-stake-keypair.json`
- `solana validators` displays the current active stake of all validators, including yours
- `solana stake-history` shows the history of stake warming up and cooling down over recent epochs
- Look for log messages on your validator indicating your next leader slot: `[2019-09-27T20:16:00.319721164Z INFO solana_core::replay_stage] <VALIDATOR_IDENTITY_PUBKEY> voted and reset 역사증명 at tick height ####. My next leader slot is ####`
- Once your stake is warmed up, you will see a stake balance listed for your validator by running `solana validators`

## Monitor Your Staked Validator

Confirm your validator becomes a [leader](../terminology.md#leader)

- After your validator is caught up, use the `solana balance` command to monitor the earnings as your validator is selected as leader and collects transaction fees
- Solana nodes offer a number of useful JSON-RPC methods to return information about the network and your validator's participation. Make a request by using curl \(or another http client of your choosing\), specifying the desired method in JSON-RPC-formatted data. For example:

```bash
  // Request
  curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getEpochInfo"}' http://localhost:8899

  // Result
  {"jsonrpc":"2.0","result":{"epoch":3,"slotIndex":126,"slotsInEpoch":256},"id":1}
```

Helpful JSON-RPC methods:

- `getEpochInfo` \[An epoch\] (../ terminology.md # epoch)는 시간, 즉 \[슬롯\] (../ terminology.md # slot)의 수입니다. 어느 \[리더 일정\] (../ terminology.md # leader-schedule)이 유효합니다. 이것은 현재 시대가 무엇이며 클러스터가 얼마나 멀리 떨어져 있는지 알려줍니다.
- `getVoteAccounts` 이것은 현재 밸리데이터이 보유하고있는 활성 지분의 양을 알려줍니다. 밸리데이터 지분의 %는 에포크 경계에서 활성화됩니다. Solana의 스테이킹에 대한 자세한 내용은 \[여기\] (../ cluster / stake-delegation-and-rewards.md)에서 확인할 수 있습니다.
- `getLeaderSchedule` 주어진 순간에 네트워크는 원장 항목을 생성하기 위해 오직 한 명의 밸리데이터만 기대합니다. \[원장 항목을 생성하기 위해 현재 선택된 밸리데이터\] (../ cluster / leader-rotation.md # leader-rotation)를 "리더"라고합니다. 이렇게하면 현재 활성화 된 스테이킹에 대한 전체 리더 일정 \ (슬롯 별 기준 \)이 반환되며 ID pubkey는 여기에 1 회 이상 표시됩니다.

## Deactivating Stake

클러스터에서 유효성 검사기를 분리하기 전에비활성화하면,이전에 실행하여 위임 된 지분을

```bash
solana deactivate-stake ~/validator-stake-keypair.json
```

지분 즉시 해제되지대신 스테이킹 워밍업과 비슷한 방식으로 냉각됩니다. 스테이킹가 냉각되는 동안 밸리데이터은 클러스터에 연결되어 있어야합니다. 식히는 동안 귀하의 스테이킹는 계속해서 보상을 얻습니다. 스테이킹 쿨 다운 후에야 밸리데이터를 끄거나 네트워크에서 철회하는 것이 안전합니다. 쿨 다운은 진행중인 지분과 지분 규모에 따라 완료하는 데 여러 세대가 걸릴 수 있습니다.

스테이킹 계정은 한 번만 사용할 수 있으므로 비활성화 후 cli의`withdraw-stake` 명령을 사용하여 이전에 스테이킹 한 램프를 복구하십시오.
