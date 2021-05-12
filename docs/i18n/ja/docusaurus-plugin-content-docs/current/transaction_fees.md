---
title: Transaction Fees
---

**（変わる可能性があります。）**

Each transaction sent through the network, to be processed by the current leader validation-client and confirmed as a global state transaction, contains a transaction fee. 取引手数料は、例えば、Solana の経済設計で多くの利点を提供します。

- ステートトランザクションを処理するために必要な CPU/GPU リソースに対する検証ネットワークに対する単位補償を提供します。
- トランザクションに実際のコストを導入することで、ネットワークのスパムを削減できます
- 以下に記載されているように、プロトコルによって取得される最小手数料額を通じて、ネットワークの潜在的な長期的な経済的安定性を提供します。

Network consensus votes are sent as normal system transfers, which means that validators pay transaction fees to participate in consensus.

現在のブロックチェーン経済の多くは(ビットコインやイーサリアムといった)短期的にはプロトコルベースの報酬に依存しており、プロトコル由来の報酬が失効しても、長期的には取引手数料による収益が経済を支えることを前提としています。 In an attempt to create a sustainable economy through protocol-based rewards and transaction fees, a fixed portion (initially 50%) of each transaction fee is destroyed, with the remaining fee going to the current leader processing the transaction. 予定されているグローバルなインフレ率は、上述のプロセスを経て、検証クライアントに分配される報酬の源となります。

トランザクション手数料は、最近の履歴スループットに基づいてネットワーククラスタによって設定されます。 [混雑駆動手数料](implemented-proposals/transaction-fees.md#congestion-driven-fees) を参照してください。 This minimum portion of each transaction fee can be dynamically adjusted depending on historical _signatures-per-slot_. このようにして、プロトコルは最低料金を利用して、望ましいハードウェア利用率を目指すことができます。 By monitoring a protocol specified _signatures-per-slot_ with respect to a desired, target usage amount, the minimum fee can be raised/lowered which should, in turn, lower/raise the actual _signature-per-slot_ per block until it reaches the target amount. この調整プロセスは、ビットコインプロトコルの難易度調整アルゴリズムに似ていると考えられますが、この場合は、取引処理ハードウェアの使用量を望ましいレベルに導くために、最低取引手数料を調整しています。

前述のように、各取引手数料の固定割合は破棄されます。 The intent of this design is to retain leader incentive to include as many transactions as possible within the leader-slot time, while providing an inflation limiting mechanism that protects against "tax evasion" attacks \(i.e. side-channel fee payments\).

さらに、フォークを選ぶ際には燔祭料も考慮する必要があります。 悪意のある検閲リーダーがいる PoH フォークの場合、検閲によって失われた手数料のために、破壊された手数料の合計は、同等の誠実なフォークよりも少なくなると予想されます。 もし検閲を行うリーダーがこれらの失われたプロトコル料を補償しようとするならば、彼らは自分のフォークで焼かれた料を自分で交換しなければならず、その結果、そもそも検閲を行うインセンティブが低下する可能性があります。
