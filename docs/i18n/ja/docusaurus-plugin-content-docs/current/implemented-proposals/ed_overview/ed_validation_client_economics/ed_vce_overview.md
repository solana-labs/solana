---
title: 検証クライアント経済
---

**（変わる可能性があります。） 最新の経済的議論は Solana フォーラムでフォローしてください: https://forums.solana.com**

Validator-clients は、staked tokens に分配されたインフレータブルな報酬に対して手数料を請求することができます。 この報酬は、ある PoH の状態を検証し、投票するためにコンピュートリソース\(CPU+GPU\) を提供することに対するものです。 これらのプロトコルベースの報酬は、トークンの総供給量に応じて、アルゴリズムによるディスインフレーションのスケジュールで決定されます。 このネットワークは、年間約 8％のインフレ率でスタートし、長期的に安定した 1.5％のインフレ率に達するまで、毎年 15％ずつ減少していくと予想されていますが、これらのパラメータはまだコミュニティによって確定されていません 発行されたトークンは、参加しているバリデータに分割して分配され、そのうち約 95％がバリデータへの報酬に充てられます(残りの 5％は財団の運営費に充てられます) Because the network will be distributing a fixed amount of inflation rewards across the stake-weighted validator set, the yield observed for staked tokens will be primarily a function of the amount of staked tokens in relation to the total token supply.

さらに、バリデータクライアントは、状態検証取引による手数料で収益を得ることができます。 わかりやすくするために、以下では検証クライアントの収益配分の設計と動機を、状態認証プロトコルに基づく報酬と状態認証取引の手数料と賃貸料に分けて説明します。
