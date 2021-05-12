---
title: インフレスケジュール
---

**（変わる可能性があります。） 最新の経済的議論は Solana フォーラムでフォローしてください: https://forums.solana.com**

バリデータクライアントは、ソラナネットワークの中で 2 つの機能的役割を持っています:

- その PoH の現在のグローバルな状態を検証\(vote\) します。
- その間、未処理のトランザクションを収集し、PoH に組み込む責任を負い、ネットワークのグローバルな状態を更新し、チェーンの継続性を提供します。

これらのサービスに対するバリデータ・クライアントの報酬は、ソラナの各エポックの終了時に分配されることになっています。 前述したように、バリデータクライアントへの報酬は、各バリデータのステーキングウェイトに比例して分配されるプロトコルベースの年間インフレ率(下記参照) と、各リーダのローテーション中に利用可能なリーダ請求のトランザクションフィーを介して提供されます。 つまり、 あるバリデータクライアントがリーダーに選出されている間は、各トランザクションフィーの一部を保持する機会があり、それからプロトコルで指定された金額を差し引いた金額が破棄されます( [Validation-client State Transaction Fees](ed_vce_state_validation_transaction_fees.md)\を参照してください。)

検証依頼者が受けたエポックあたりの実効プロトコールベースの年利率(％) は、次の関数です。

- 事前に決定されたディスインフレ発行スケジュールから導出された現在の世界インフレ率("[Validation-client Economics](ed_vce_overview.md)を参照してください)
- 現在の総循環供給量のうち、ステーキングされた SOL の割合。
- 検証サービスによって請求された手数料は
- あるバリデータの過去のエポックでのアップタイム/参加率[バリデータが投票する機会を持っていた空きスロットの割合]。

最初の要素はプロトコルパラメータのみの関数であり(つまり、特定のエポックにおけるバリデータの行動に依存しない)、早期の参加を促し、明確な金銭的安定性を提供し、ネットワーク内の最適なセキュリティを提供するように設計されたグローバルな検証報酬スケジュールを実現します。

As a first step to understanding the impact of the _Inflation Schedule_ on the Solana economy, we’ve simulated the upper and lower ranges of what token issuance over time might look like given the current ranges of Inflation Schedule parameters under study.

特に：

- _Initial Inflation Rate_: 7-9%
- _Dis-inflation Rate_: -14-16%
- _Long-term Inflation Rate_: 1-2%

これらの範囲を使用して、多くの可能なインフレーションスケジュールをシミュレートすることで、時間の経過とともにインフレを調べることができます。

![](/img/p_inflation_schedule_ranges_w_comments.png)

上記のグラフでは、各パラメータの貢献度を示すために、範囲の平均値が識別されます。 From these simulated _Inflation Schedules_, we can also project ranges for token issuance over time.

![](/img/p_total_supply_ranges.png)

Finally we can estimate the _Staked Yield_ on staked SOL, if we introduce an additional parameter, previously discussed, _% of Staked SOL_:

%~\text{SOL Staked} = \frac{\text{Total SOL Staked}}{\text{Total Current Supply}}

In this case, because _% of Staked SOL_ is a parameter that must be estimated (unlike the _Inflation Schedule_ parameters), it is easier to use specific _Inflation Schedule_ parameters and explore a range of _% of Staked SOL_. 以下の例では、上記で検討したパラメータ範囲の中央を選択しています。

- _Initial Inflation Rate_: 8%
- _Dis-inflation Rate_: -15%
- _Long-term Inflation Rate_: 1.5%

The values of _% of Staked SOL_ range from 60% - 90%, which we feel covers the likely range we expect to observe, based on feedback from the investor and validator communities as well as what is observed on comparable Proof-of-Stake protocols.

![](/img/p_ex_staked_yields.png)

Again, the above shows an example _Staked Yield_ that a staker might expect over time on the Solana network with the _Inflation Schedule_ as specified. This is an idealized _Staked Yield_ as it neglects validator uptime impact on rewards, validator commissions, potential yield throttling and potential slashing incidents. It additionally ignores that _% of Staked SOL_ is dynamic by design - the economic incentives set up by this _Inflation Schedule_.

### 調整されたステーキング利率

A complete appraisal of earning potential from staking tokens should take into account staked _Token Dilution_ and its impact on staking yield. For this, we define _adjusted staking yield_ as the change in fractional token supply ownership of staked tokens due to the distribution of inflation issuance. つまり、 インフレーションの正の希薄化効果です。

We can examine the _adjusted staking yield_ as a function of the inflation rate and the percent of staked tokens on the network. 様々なステーキングの分数についてプロットされています:

![](/img/p_ex_staked_dilution.png)
