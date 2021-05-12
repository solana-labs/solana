---
title: Transaction Fees
---

**Subject to change.**

Each transaction sent through the network, to be processed by the current leader validation-client and confirmed as a global state transaction, contains a transaction fee. 거래 수수료는 Solana 경제 설계에 많은 이점을 제공합니다. 예를 들면 다음과 같습니다.

- -상태 트랜잭션을 처리하는 데 필요한 CPU / GPU 리소스에 대해 유효성 검사기 네트워크에 단위 보상을 제공합니다.
- -거래에 실제 비용을 도입하여 네트워크 스팸을 줄입니다.
- -검증 클라이언트가 리더로서의 기능에 따라 제출 된 거래를 수집하고 처리하도록 인센티브를 부여하는 거래 시장의 열린 길, -아래에 설명 된대로 프로토콜로 캡처 한 트랜잭션 당 최소 수수료 금액을 통해 네트워크의 잠재적 인 장기적인 경제적 안정성을 제공합니다.

Network consensus votes are sent as normal system transfers, which means that validators pay transaction fees to participate in consensus.

현재 많은 블록체인 경제 (예 : 비트 코인, 이더 리움)는 프로토콜 기반 보상에 의존하여 단기적으로 경제를 지원하며, 거래 수수료를 통해 생성 된 수익이 장기적으로 경제를 지원할 것이라는 가정하에 프로토콜이 파생 된 보상이 만료됩니다. In an attempt to create a sustainable economy through protocol-based rewards and transaction fees, a fixed portion (initially 50%) of each transaction fee is destroyed, with the remaining fee going to the current leader processing the transaction. 예정된 글로벌 인플레이션 율은 위에서 설명한 프로세스를 통해 검증 클라이언트에게 분배되는 보상의 출처를 제공합니다.

Transaction fees are set by the network cluster based on recent historical throughput, see [Congestion Driven Fees](implemented-proposals/transaction-fees.md#congestion-driven-fees). This minimum portion of each transaction fee can be dynamically adjusted depending on historical _signatures-per-slot_. 이러한 방식으로 프로토콜은 최소 요금을 사용하여 원하는 하드웨어 사용률을 목표로 할 수 있습니다. By monitoring a protocol specified _signatures-per-slot_ with respect to a desired, target usage amount, the minimum fee can be raised/lowered which should, in turn, lower/raise the actual _signature-per-slot_ per block until it reaches the target amount. 이 조정 프로세스는 비트 코인 프로토콜의 난이도 조정 알고리즘과 유사하다고 생각할 수 있지만이 경우 최소 트랜잭션 수수료를 조정하여 트랜잭션 처리 하드웨어 사용을 원하는 수준으로 안내합니다.

앞서 언급했듯이 각 거래 수수료의 고정 비율은 파기됩니다. The intent of this design is to retain leader incentive to include as many transactions as possible within the leader-slot time, while providing an inflation limiting mechanism that protects against "tax evasion" attacks \(i.e. side-channel fee payments\).

또한, 소각 된 수수료는 포크 선택시 고려 사항이 될 수 있습니다. 악의적 인 검열 리더가있는 역사증명 포크의 경우 검열로 인해 손실 된 요금으로 인해 파괴 된 총 요금이 정직한 포크보다 적을 것으로 예상됩니다. 검열 리더가 이러한 손실 된 프로토콜 수수료를 보상하려면 포크 자체에서 소각 된 수수료를 교체해야하므로 처음에 검열에 대한 인센티브를 잠재적으로 줄일 수 있습니다.
