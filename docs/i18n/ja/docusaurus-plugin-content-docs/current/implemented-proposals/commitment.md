---
title: コミット
---

コミットメント指標は、特定のブロックにおけるネットワークの確認とステーキングレベルの指標をクライアントに提供することを目的としています。 クライアントはこの情報をもとに、独自のコミットメント指標を導き出すことができます。

# Calculation RPC

クライアントは RPC 上の`get_block_commitment(s: Signature) ->> BlockCommitment`を通じて、バリデータに署名`sの`コミットメトリクスを要求できます。 `BlockCommitment`構造体には、u64`[u64, MAX_CONFIRMATIONS]`の配列が含まれています。 この配列は、バリデータが投票した最後のブロック`M`の時点で、署名`s`を含む特定のブロック`N`のコミットメトリックを表しています。

`BlockCommitment`配列のインデックス`i`にあるエントリ`s`は、あるブロック`M`で観測された、ブロック`N`で`i`の確認に達したクラスタの`s`のトータルステークをバリデータが観測したことを意味します。 この配列には`MAX_CONFIRMATIONS`という要素があり、 1 から`MAX_CONFIRMATIONS`までの確認数の可能性を表します。

# コミットメントメトリックの計算

この`BlockCommitment`構造体の構築には、合意形成のために既に行われている計算が活用されています。 `consensus.rs`の`collect_vote_lockouts`関数は HashMap を構築し、各エントリは`(b, s)`の形をしており、`s`は銀行`b`への賭け金の額です。

この計算は、投票可能な候補銀行`b`に対して以下のように行われます。

```text
   let output: HashMap<b, Stake> = HashMap::new();
   for vote_account in b.vote_accounts {
       for v in vote_account.vote_stack {
           for a in ancestors(v) {
               f(*output.get_mut(a), vote_account, v);
           }
       }
   }
```

ここで`f`は、スロット`a`の`Stake`エントリを、投票`v`から得られるデータで修正する蓄積関数であり、また `vote_account`から得られるデータ(stake, lockout など) で修正します。 ここでの`祖先`は、現在のステータスキャッシュに存在するスロットのみを含むことに注意してください。 テータスキャッシュに存在する Bank よりも前の Bank の署名は、いずれにしても照会可能ではないので、それらの Bank はここでのコミットメントの計算には含まれません。

ここで、上記の計算を拡張して、各バンク`b`の`BlockCommitment`配列を以下のように構築することができます。

1. `BlockCommitment` 構造体を収集するために `ForkCommitmentCache` を追加します。
2. `f`を`f'`に置き換えることで、上記の計算はすべてのバンク`b`に対してもこの`BlockCommitment`を構築することになります。

1) は簡単なので、2) の詳細を説明します。

続行する前に、あるバリデータの投票アカウント `a` について、スロット `s` でのそのバリデータのローカルな確認数が `v.num_confirmations` であることに注目したいです。 ここで`v`は`a.votes`のスタックの中で `v.slot >= s`となる最小の票です(すなわち、確認数が少なくなるので > v の票を見る必要はありません)。

さて、より具体的には、上記の計算を次のように補強します。

```text
   let output: HashMap<b, Stake> = HashMap::new();
   let fork_commitment_cache = ForkCommitmentCache::default();
   for vote_account in b.vote_accounts {
       // vote stack is sorted from oldest vote to newest vote
       for (v1, v2) in vote_account.vote_stack.windows(2) {
           for a in ancestors(v1).difference(ancestors(v2)) {
               f'(*output.get_mut(a), *fork_commitment_cache.get_mut(a), vote_account, v);
           }
       }
   }
```

`f'` は次のように定義されています:

```text
    fn f`(
        stake: &mut Stake,
        some_ancestor: &mut BlockCommitment,
        vote_account: VoteAccount,
        v: Vote, total_stake: u64
    ){
        f(stake, vote_account, v);
        *some_ancestor.commitment[v.num_confirmations] += vote_account.stake;
    }
```
