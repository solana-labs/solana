---
title: 낙관적 확인
---

## 기초 요소

이러한 "전환 투표"를`X'`로 수행하려면 전환 증명`SP (Vote (X, S), Vote (X ', S'))`가 지분의`> 1 / 3`을 보여 주어야합니다. 이 밸리데이터의 최근 투표 인`S.last`에서 잠겼습니다. 이`> 1 / 3`을`낙관적 유권자`세트의 밸리데이터 세트가 지분의`> 2 / 3`로 구성된다는 사실과 결합하면`낙관적 유권자`중 하나 이상의 낙관적 밸리데이터`W`를 의미합니다. 세트가 투표를 제출해야합니다 (전환 증명의 정의를 기억하세요),`V`의 슬롯`X '에 대한 전환 증명에 포함 된`Vote (X_w, S_w)`, 여기서`S_w`에는 슬롯이 포함됩니다.`s`는 다음과 같습니다.

투표`vote (X, S)`가 주어지면`S.last == vote.last`가`S`의 마지막 슬롯이되도록합니다.

이제 몇 가지 "낙관적 슬래싱"슬래싱 조건을 정의합니다. 이에 대한 직관은 아래에 설명되어 있습니다.

- `Vote (X ', S')`를 밸리데이터`V`가 만든`Optimistic Votes`의 투표로 지정합니다. 해당 세트의 정의 (모든 투표는 낙관적으로`B`로 확인 됨),`X '<= B <= S'.last` (위의 다이어그램 참조).

```text
                                  +-------+
                                  |       |
                        +---------+       +--------+
                        |         |       |        |
                        |         +-------+        |
                        |                          |
                        |                          |
                        |                          |
                    +---+---+                      |
                    |       |                      |
                X   |       |                      |
                    |       |                      |
                    +---+---+                      |
                        |                          |
                        |                      +---+---+
                        |                      |       |
                        |                      |       |  X'
                        |                      |       |
                        |                      +---+---+
                        |                          |
                        |                          |
                        |                          |
                        |                          |
                        |                      +---+---+
                        |                      |       |
                        |                      |       |  S'.last
                        |                      |       |
                        |                      +-------+
                        |
                    +---+---+
                    |       |
                 s  |       |
                    |       |
                    +---+---+
                        |
                        |
                        |
                        |
                    +---+---+
                    |       |
             S.last |       |
                    |       |
                    +-------+
```

(슬래시 가능한 투표의 예 vote (X ', S') 및 vote (X, S))

In the diagram above, note that the vote for `S.last` must have been sent after the vote for `S'.last` (due to lockouts, the higher vote must have been sent later). 따라서 투표 순서는`X ... S'.last ... S.last` 여야합니다. 즉,`S'.last`에 대한 투표 후 유효성 검사기가 일부 슬롯`s> S'.last> X`에서 다른 포크로 다시 전환해야합니다. Thus, the vote for `S.last` should have used `s` as the "reference" point, not `X`, because that was the last "switch" on the fork.

이를 시행하기 위해 "Optimistic Slashing"슬래싱 조건을 정의합니다. 동일한 밸리데이터이 두 개의 서로 다른 투표`vote (X, S)`와`vote (X ', S')`를 주면 다음을 충족해야합니다.

- `X > X'` implies `X > S'.last` and `S.last > S'.last` and for all `s` in `S'`, `s + lockout(s) < X`
- 'S'의 모든 's'는 서로의 조상 / 하위, 'S'의 모든 's'는 서로의 조상 / 하위,
-
- -루팅 된`B'` 또는`B'`의 자손 -`Vote (X, S)`형식의`v`도 제출했으며`X <= B <= v.last`입니다.
- `X' > X` implies `X' > S.last` and `S'.last > S.last` and for all `s` in `S`, `s + lockout(s) < X'`
- `-<code>s '+ s'.lockout> S.last`

(마지막 두 규칙은 범위가 겹칠 수 없음을 의미합니다.) : 그렇지 않으면 유효성 검사기가 슬래시됩니다.

`X <= S.last`,`X '<= S'.last`

`SP (old_vote, new_vote)`-밸리데이터의 최근 투표 인`old_vote`에 대한 "전환 증명"입니다. 이러한 증명은 밸리데이터가 "참조"슬롯을 전환 할 때마다 필요합니다 (위의 투표 섹션 참조). 스위칭 증명에는`old_vote`에 대한 참조가 포함되어 있으므로 해당`old_vote`의 "범위"가 무엇인지에 대한 기록이 있습니다 (이 범위에서 다른 충돌 스위치를 슬래시 가능하게 만들기 위해). 이러한 스위치는 여전히 잠금을 존중해야합니다.

`Range (vote)`-투표`v = vote (X, S)`가 주어지면`Range (v)`를 슬롯`[X, S.last]`의 범위로 정의합니다.

The proof is a list of elements `(validator_id, validator_vote(X, S))`, where:

1. 모든 밸리데이터 ID의 '> 1/3'지분의 합

2. 각`(validator_id, validator_vote (X, S))`에 대해`S`에 슬롯`s`가 있습니다. 여기서 _ a.`s`는`validator_vote.last`와`둘 다의 공통 조상이 아닙니다. old_vote.last` 및`new_vote.last`. _ b. `s`는`validator_vote.last`의 자손이 아닙니다. \* c. `s + s.lockout ()> = old_vote.last` (유효성 검사기가 슬롯`old_vote.last`의 슬롯`s`에서 여전히 잠겨 있음을 나타냄).

스위칭 증명은 네트워크의`> 1 / 3`이 슬롯`old_vote.last`에서 잠겨 있음을 보여줍니다.

## 정의 :

증명은`(validator_id, validator_vote (X, S))`요소의 목록입니다.

유효한 스위칭 증명이없는 스위칭 포크는 슬래시 가능합니다.

Reverted - A block `B` is said to be reverted if another block `B'` that is not a parent or descendant of `B` was finalized.

## 보장 :

완료 됨-하나 이상의 올바른 유효성 검사기가 'B'또는 'B'의 하위 항목을 루팅 한 경우 블록 'B'가 완료되었다고합니다.

## 증명 :

되돌림- 'B'의 부모 또는 하위 항목이 아닌 다른 블록 'B'가 완료되면 블록 'B'가 되돌려 진다고합니다.

- Another block `B'` that is not a parent or descendant of `B` was finalized.
- No validators violated any slashing conditions.

`낙관적 확인`의 정의에 따르면 이는 밸리데이터의`> 2 / 3`가 각각`Vote (X, S)`형식의`v`를 보였음을 의미합니다. 여기서`X <= B <= v.last` . 따라서`Optimistic Votes`를`Optimistic Validators`가 만든 투표 세트로 정의합니다. 따라서`Optimistic Votes`를`Optimistic Validators`가 만든 투표 세트로 정의합니다.

모순을 위해 블록 'B'가 일부 'n'에 대해 일부 슬롯 'B + n'에서 '낙관적 확인'을 달성했다고 가정합니다.

밸리데이터`V`는 슬롯`X '로 전환하는 증거에`Vote (X_w, S_w)`투표를 포함했기 때문에 밸리데이터`V'`가`Vote (X_w, S_w)`** 전에 ** 투표를 제출했음을 암시합니다. 밸리데이터`V`는 슬롯`X'`,`Vote (X ', S')`에 대한 전환 투표를 제출했습니다.

### 정리 1 :

`Claim:` Given a vote `Vote(X, S)` made by a validator `V` in the `Optimistic Validators` set, and `S` contains a vote for a slot `s` for which:

- `s '+ s'.lockout> B`
- `s + s. 잠금> B`, -`s`는`B`의 조상 또는 후손이 아닙니다.

then `X > B`.

```text
                                  +-------+
                                  |       |
                        +---------+       +--------+
                        |         |       |        |
                        |         +-------+        |
                        |                          |
                        |                          |
                        |                          |
                        |                      +---+---+
                        |                      |       |
                        |                      |       |  X'
                        |                      |       |
                        |                      +---+---+
                        |                          |
                        |                          |
                        |                      +---+---+
                        |                      |       |
                        |                      |       |  B (Optimistically Confirmed)
                        |                      |       |
                        |                      +---+---+
                        |                          |
                        |                          |
                        |                          |
                        |                      +---+---+
                        |                      |       |
                        |                      |       |  S'.last
                        |                      |       |
                        |                      +-------+
                        |
                    +---+---+
                    |       |
                 X  |       |
                    |       |
                    +---+---+
                        |
                        |
                        |
                        |
                        |
                        |
                    +---+---+
                    |       |
            S.last  |       |
                    |       |
                    +---+---+
                        |
                        |
                        |
                        |
                    +---+---+
                    |       |
      s + s.lockout |       |
                    +-------+
```

` Claim :``Optimistic Validators ` 세트에서`V` 밸리데이터가 만든`Vote (X, S)`투표가 주어졌고`S`는 다음과 같은 슬롯`s`에 대한 투표를 포함합니다.

`Proof` :`Vote (X, S)`를`Optimistic Votes` 세트의 투표로 지정합니다. 그런 다음 정의에 따라 "낙관적으로 확인 된"블록`B`,`X <= B <= S.last`가 주어집니다.

그런 다음`X> B`.

`Case X == X'`:

`s`를 고려하십시오. `s`는`B`의 조상이나 후손이 아니라`s`가`B`의 조상이라고 가정하기 때문에 우리는`s! = X`를 알고 있습니다. Because `S'.last` is a descendant of `B`, this means `s` is also not an ancestor or descendant of `S'.last`. Then because `S.last` is descended from `s`, then `S'.last` cannot be an ancestor or descendant of `S.last` either. 이것은 "Optimistic Slashing"규칙에 의해`X! = X'`를 의미합니다.

! = X'</code>, 아래와 같이 :

`케이스 X == X'` :

위의 가정에서`s + s.lockout> B> X'`. `s`는`X'`의 조상이 아니기 때문에이 밸리데이터이`Vote (X ', S' ') 형식의 일부 투표로`X'`에 대한 전환 투표를 처음 제출하려고 시도했을 때 잠금이 위반되었을 것입니다.`.

`케이스 X <X'` :

### 정리 2 :

직관적으로 이것은`Vote (X, S)`가`Vote (X ', S')` "전에"이루어 졌음을 의미합니다.

`Claim`: For any vote `Vote(X, S)` in the `Optimistic Votes` set, it must be true that `B' > X`

```text
                                +-------+
                                |       |
                       +--------+       +---------+
                       |        |       |         |
                       |        +-------+         |
                       |                          |
                       |                          |
                       |                          |
                       |                      +---+---+
                       |                      |       |
                       |                      |       |  X
                       |                      |       |
                       |                      +---+---+
                       |                          |
                       |                          |
                       |                      +---+---+
                       |                      |       |
                       |                      |       |  B (Optimistically Confirmed)
                       |                      |       |
                       |                      +---+---+
                       |                          |
                       |                          |
                       |                          |
                       |                      +---+---+
                       |                      |       |
                       |                      |       |  S.last
                       |                      |       |
                       |                      +-------+
                       |
                   +---+---+
                   |       |
    B'(Finalized)  |       |
                   |       |
                   +-------+
```

`Proof` : 모순을 위해 "Optimistic Validators"세트의 Validator`V`가 그러한 투표`Vote (X, S)`를했다고 가정합니다.`S`는 조상이 아닌 슬롯`s`에 대한 투표를 포함합니다. 또는`B`의 하위 항목, 여기서`s + s.lockout> B`이지만`X <= B`입니다.

회상 'B'는 '낙관적으로 확인 된'블록 'B'와 다른 포크에서 완성 된 블록입니다.

- `B' != X`
- = X</code> -`B'`는`X`의 부모가 아닙니다.

`Claim` :`Optimistic Votes` 세트의`Vote (X, S)`투표에 대해`B '> X`가 참이어야합니다.

`Case B '<X` : 이것이 잠금 위반임을 보여줄 것입니다. 위에서 살펴보면 'B'가 'X'의 부모가 아님을 알 수 있습니다. 그러면`B'`가 루팅되고`B'`가`X`의 부모가 아니기 때문에 밸리데이터은`B'`에서 내려 오지 않는 상위 슬롯`X`에 투표 할 수 없었어야합니다.

### 안전 증명 :

`X`는`B`의 부모이고`B'`는`B`의 부모 또는 조상이 아니기 때문에 :

먼저 'B'가 루팅되기 위해서는 'B' '또는'B '의 후손에 투표 한'> 2/3 '지분이 있어야합니다. `Optimistic Validator` 세트에 스테이킹 된 밸리데이터의`> 2 / 3`도 포함되어 있다는 점을 감안할 때 스테이킹 된 밸리데이터의`> 1 / 3`을 따릅니다.

- Rooted `B'` or a descendant of `B'`
- 위에서`S.last> = B> = X`이므로 이러한 모든 "전환 투표"에 대해`X_v> B`입니다.

이제`B'` <`X`를 고려하십시오.

정의에 따라 'B'를 루트하려면 'Delinquent'의 각 유효성 검사기 'V'가 각각 'Vote (X_v, S_v)'형식의 "전환 투표"를 수행해야합니다. 여기서 :

- `'S_v.last'는 'B'의 후손이므로 'B'의 후손이 될 수 없습니다.`
- 'S_v.last'는 'B'의 후손이 아니므로 'X_v'는 'B'의 후손 또는 조상이 될 수 없습니다.
- = X</code> (위에서`B`의 후손 또는 조상일 수 없음)이므로 슬래시 규칙에 따라`X_v> S.last`를 알 수 있습니다.

이제 '낙관적 밸리데이터'집합에있는 밸리데이터 중 하나 이상이 슬래싱 규칙을 위반했음을 표시하는 것을 목표로합니다.

`X == X'`는`S`가`S '의 부모이거나`S'`가`S`의 부모임을 의미합니다. -`X '> X`는`X'> S.last`및`S'.last> S.last`를 의미하고`S`의 모든`s`에 대해`s + lockout (s) <X'`를 의미합니다. -`X> X'`는`X> S'.last`및`S.last> S'.last`를 의미하며`S '의 모든`s`에 대해`s + lockout (s) <X`를 의미합니다.

이제이 모든 "전환 투표"를 제때에 주문하고,`V`가 이러한 "전환 투표"`Vote (X ', S')`를 처음 제출 한`Optimistic Validators`의 밸리데이터가되도록합니다. 여기서`X '> B`. 우리는 위에서부터 모든 체납 검증자가 그러한 투표를 제출해야하고 체납 검증자가 '낙관적 검증 자'의 하위 집합이라는 것을 알고 있기 때문에 그러한 검증자가 존재한다는 것을 알고 있습니다.

'Delinquent'세트를 위의 기준을 충족하는 유효성 검사기 세트로 설정합니다.

여기서`X <= B <= S .last`,`X '<= B <= S'.last`,`X == X'`, 그렇지 않으면 "Optimistic Slashing"조건이 위반됩니다 (각 투표의 "범위"는`B`에서 겹칩니다. ).

`vote (X, S)`-투표는이 검증자가 전환 증명으로 투표 한이 포크의 ** 최신 ** 조상 인 '참조'슬롯 'X'로 증가됩니다. 검증 인이 서로의 후손 인 연속 투표를하는 한 모든 투표에 동일한 'X'를 사용해야합니다. 유효성 검사기가 이전의 하위 슬롯이 아닌`s`에 투표하면`X`가 새 슬롯`s`로 설정됩니다. 그러면 모든 투표는`vote (X, S)`형식이됩니다. 여기서`S`는 투표되는 슬롯`(s, s.lockout)`의 정렬 된 목록입니다. Combine this `>1/3` with the fact that the set of validators in the `Optimistic Voters` set consists of `> 2/3` of the stake, implies at least one optimistic validator `W` from the `Optimistic Voters` set must have submitted a vote (recall the definition of a switching proof),`Vote(X_w, S_w)` that was included in validator `V`'s switching proof for slot `X'`, where `S_w` contains a slot `s` such that:

- `s` is not a common ancestor of `S.last` and `X'`
- `s`는`S.last`의 후손이 아닙니다.
- `S_v.last> B'`

정의에 따라이 체납 밸리데이터`V`는`낙관적 투표`에서`투표 (X, S)`를 만들었습니다.이 세트의 정의 (낙관적으로 확인 된`B`)에 따르면`S.last> = B > = X`.

- `s`는`B`와`X'`의 공통 조상이 아닙니다.
- `s' + s'.lockout > B`

'V'의 스위칭 증명에 포함되었습니다.

이제`W`는`낙관적 유권자`의 일원이기 때문에 위`정의 1`에 의해`W`,`Vote (X_w, S_w)`의 투표가 주어집니다. 여기서`S_w`는 슬롯`s` 여기서`s + s.lockout> B`이고`s`는`B`의 조상이 아닌 경우`X_w> B`입니다.

`Vote (X, S)`를 밸리데이터`V`가 만든`Optimistic Votes`에서 고유 한 투표로 지정합니다 (`S.last` 최대화).

`X '> B> = X`,`X'> X`이므로`Vote (X, S)`가 주어지면 "Optimistic Slashing"규칙에 따라`X '> S.last`가됩니다.
