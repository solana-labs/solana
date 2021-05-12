---
title: Optimistic Confirmation
---

## Primitives

`vote(X, S)` - Votes will be augmented with a "reference" slot, `X` which is the **latest** ancestor of this fork that this validator voted on with a proof of switching. As long as the validator makes consecutive votes that are all descended from each other, the same `X` should be used for all those votes. When the validator makes a vote for a slot `s` that is not descended from the previous, `X` will be set to the new slot `s`. All votes will then be of the form `vote(X, S)`, where `S` is the sorted list of slots `(s, s.lockout)` being voted for.

Given a vote `vote(X, S)`, let `S.last == vote.last` be the last slot in `S`.

Now we define some "Optimistic Slashing" slashing conditions. The intuition for these is described below:

- `Intuition`: If a validator submits `vote(X, S)`, the same validator should not have voted on a different fork that "overlaps" this fork. More concretely, this validator should not have cast another vote `vote(X', S')` where the range `[X, S.last]` overlaps the range `[X', S'.last]`, `X != X'`, as shown below:

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

(Example of slashable votes vote(X', S') and vote(X, S))

In the diagram above, note that the vote for `S.last` must have been sent after the vote for `S'.last` (due to lockouts, the higher vote must have been sent later). Thus, the sequence of votes must have been: `X ... S'.last ... S.last`. This means after the vote on `S'.last`, the validator must have switched back to the other fork at some slot `s > S'.last > X`. Thus, the vote for `S.last` should have used `s` as the "reference" point, not `X`, because that was the last "switch" on the fork.

To enforce this, we define the "Optimistic Slashing" slashing conditions. Given any two distinct votes `vote(X, S)`and `vote(X', S')` by the same validator, the votes must satisfy:

- `X <= S.last`, `X' <= S'.last`
- Todos os `s` em `S` são antepassados/descendentes um do outro, todos os `s` em `S'` são antepassados/descendentes um do outro,
-
- `X == X'` implica que `S` é pai de `S'` ou `S'` é pai de `S`
- `X' > X` implica em `X' > S. ast` e `S'.last > S. ast` e para todos os `s` em `S`, `s + bloqueio(s) < X'`
- `X' > X` implica em `X' > S. ast` e `S'.last > S. ast` e para todos os `s` em `S`, `s + bloqueio(s) < X'`

(As duas últimas regras implicam que os intervalos não podem se sobrepor): Caso contrário, o validador é cortado.

`Estande (votação)` - Dado um voto `v = voto (X, S)`, defina `Intervalo (v)` como o intervalo de espaços `[X, S. ast]`.

`SP(old_vote, new_vote` - Esta é a "Mudança de prova" para `old_vote`, a última votação do validador. Tal prova é necessária sempre que um validador alterna seu slot de "referência" (veja a seção de votação acima). A prova de mudança inclui uma referência a `old_vote`,, para que haja um registro de qual era o intervalo de `old_vote` (para fazer outros interruptores conflitantes nesta faixa de controle). Tal mudança deve continuar a respeitar os bloqueios.

Uma prova de mudança mostra que `> 1/3` da rede está bloqueado no slot `old_vote.last`.

A prova é uma lista de elementos `(validator_id, validator_vote(X, S))`, onde:

1. A soma das participações de todos os validadores de `> 1/3`

2. Para cada `(validator_id, validator_vote(X, S))`, há algum slot `s` em `S` onde: _ a.`s` não é um ancestral comum de ambos `validator_vote ast` e `antigo_vote.last` and `new_vote.last`. _ b. `s` não é um descendente de `validator_vote.last`. \* c. `s + s.lockout() >= old_vote ast` (implica que o validador ainda está bloqueado no slot `s` no slot `old_vote.last`).

Mudar bifurcações sem uma prova de troca válida é baralhado.

## Definições:

Confirmação Otimista - Um bloco `B` depois diz que alcançou "confirmação otimista" se `>2/3` de participação votaram com votos `v` onde `Intervalo(v)` para cada um desses `v` inclui `B. lot`.

Finalizado - dizem que um bloco `B` será finalizado se pelo menos um validador correto tiver rooteado `B` ou um descendente de `B`.

Revertido - Dizem que um bloco `B` é revertido se outro bloco `B'` que não é pai ou descendente de `B` foi finalizado.

## Garantias:

Um bloco `B` que atingiu a confirmação otimista não será revertida, a menos que pelo menos um validador seja cortado.

## Comprovar:

Assuma a bem da contradição, um bloco `B` alcançou `confirmação otimista` em algum slot `B + n` para alguns `n`, e:

- Outro bloco `B'` que não é pai ou descendente de `B` foi finalizado.
- Nenhum validador violou quaisquer condições de corte.

Por definição de `confirmação otimista`, isto significa que `> 2/3` de validadores têm cada um mostrado algum voto `v` do formulário `Voto(X, S)` onde `X <= B <= v. último`. Chame este conjunto de validadores `Otimizar validadores`.

Now given a validator `v` in `Optimistic Validators`, given two votes made by `v`, `Vote(X, S)` and `Vote(X', S')` where `X <= B <= S.last`, and `X' <= B <= S'.last`, then `X == X'` otherwise an "Optimistic Slashing" condition is violated (the "ranges" of each vote would overlap at `B`).

Thus define the `Optimistic Votes` to be the set of votes made by `Optimistic Validators`, where for each optimistic validator `v`, the vote made by `v` included in the set is the `maximal` vote `Vote(X, S)` with the greatest `S.last` out of any votes made by `v` that satisfy `X <= B <= S.last`. Porque sabemos do acima `X` para todos os votos feitos por `v` é únicos, nós sabemos que existe um tal `máximo` voto.

### Lemma 1:

`Reivindicar:` Dado um voto `Voto (X, S)` feito por um validador `V` no conjunto `Otimizar validadores`, e `S` contém uma votação para um slot `s` para que:

- `s + s.lockout > B`, ,
- `s` não é um ancestral ou descendente de `B`,

então `X > B`. .

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
 
Text
XPath: /pre[2]/code
File: optimistic_confirmation.md
```

`Prova`: Assumir por causa de contradição, um validador `V` do conjunto "validador otimista" fez um voto `Votar(X, S)` onde `S` contém uma votação para um slot `s` não é um ancestral ou decente de `B`, , onde `s + s. ockout > B`, mas `X <= B`.

Deixe `voto (X', S')` ser o voto em `Otimizar votos` definidos por validador `V`. Por definição do conjunto (todos os votos confirmados otimistamente `B`), `X' <= B <= S'. ast` (ver diagrama acima).

Isto implica que é assumido acima de `X <= B`, então `X <= S'. ast`, pelas regras de corte, ou `X == X'` ou `X < X'` (o contrário iria se sobrepor ao intervalo `(X', S'. ast)`).

`Caso X == X'`:

Considere `s`. Nós conhecemos `s ! X` porque é assumido que `s` não é um ancestral ou descendente de `B`, e `X` é um ancestral de `B`. Porque `S'. ast` é uma descendente de `B`, isto significa `s` também não é um ancestral ou descendente de `S. mínimo de`. Porque `S'. ast` é umadescendente de `B`, isto significa `s` também não é um ancestral ou descendente de `S. mínimo de`. Isso implica em `X != X'` pelas regras de "Otimiztic Slashing".

`Caso X == X'`:

Intuitivamente, isso implica que `Voto (X, S)` foi feito "antes" `Voto (X', S')`.

Na suposição acima, `s + s.lockout > B > X'`. Porque `s` não é um ancestral de `X'`, lockouts teria sido violado quando este validador primeiro tentou enviar uma votação de mudança para `X'` com algum voto do formulário voto `(X', S'')`.

Como nenhum desses casos é válido, a suposição deve ser inválida, e a reivindicação é provada.

### Lemma 2:

Chamar de volta `B'` foi o bloco finalizado em um fork diferente de "otimisticamente" confirmado" bloco `B`.

`Reivindicar`: Para qualquer voto `Voto (X, S)` nos `Votos Otimistas` definidos, deve ser verdade que `B' > X`

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

`Prova`: Permita que `Vote (X, S)` seja um voto nos `Votos Otimistas` definidos. Em seguida, por definição , dado o bloco "otimista confirmado" `B`, `X <= B <= S.last`.

Porque `X` é um pai de `B`, e `B'` não é pai ou ancestral de `B`, então:

- `B' != X`
- `B'` não é pai de `X`

Agora considere se `B'` < `X`:

`Caso B' < X`: Nós mostraremos que isso é uma violação do bloqueio. `B'` não é pai de `X`. Então porque `B'` foi rooteado, e `B'` não é pai de `X`, então o validador não deveria ter conseguido votar no slot superior `X` que não desce de `B'`.

### Comprovante de segurança:

Agora temos o objetivo de mostrar pelo menos um dos validadores no conjunto `Otimizar dos validadores` violou uma regra de corte.

Primeiro observe que a ordem de `B'` foi enraizada, deve ter sido uma participação `> 2/3` que votou em `B'` ou um descendente de `B'`. Dado que o conjunto `Validador Otimista` também contém `> 2/3` dos validadores staked, segue que `> 1/3` dos validadores acionados:

- Administrado `B'` ou descendente de `B'`
- Também enviou uma votação `v` do formulário `Voto (X, S)` onde `X <= B <= v.last`.

Deixe o conjunto `Delinquent` ser o conjunto de validadores que atendem ao critério acima .

Por definição, a fim de raiz `B'`, cada validador `V` em `Delinquent` deve ter feito algum "voto de mudança" do formulário `Voto (X_v, S_v)` onde:

- `D_v.last > B'`
- `S_v.last` é um descendente de `B'`, então não pode ser descendente de `B`
- Porque `S_v. ast` não é um descendente de `B`, então `X_v` não pode ser um descendente ou ancestral de `B`.

Por definição, este validador delinquente `V` também fez um voto `Vote(X, S)` nos `Votos Otimistas` onde por definição esse conjunto (otimista confirmado `B`), nós conhecemos `S. ast >= B >= X`.

Por `Lemma 2` conhecemos `B' > X`, e de cima `S_v. ast > B'`então `S_v.last > X`. Porque `X_v ! X` (não pode ser um descendente ou ancestral de `B` de cima), então pelas regras de corte sabemos `X_v > S. mires`. De cima, `S.last >= B >= X` assim para todos os "mudar de votos", `X_v > B`.

Agora, ordenando todos esses "mudando de votos" a tempo, let `V` para ser o validador em `Otimizar Validadores` que primeiro submeteram um "voto de troca" `Voto(X', S')`, onde `X' > B`. Sabemos que tal validador existe porque sabemos acima que todos os validadores delinquentes devem ter enviado tal votação e os validadores delinquentes são um subconjunto dos `Validadores Otimistas`.

Deixe `votar(X, S)` é o voto único em `Votos Otimistas` feitos pelo validador `V` (maximizando `S. ast`).

Dado `Voto (X, S)` porque `X' > B >= X`, então `X' > X`, então pelas regras "Otimiztic Slashing " `X' > S. último`.

Para realizar tal "voto de mudança" para `X'`, uma prova de mudança `SP(X, S), Voto (X', S'))` deve mostrar `> 1/3` de participação sendo bloqueada na última votação deste validador `S ast`. Combine this `>1/3` with the fact that the set of validators in the `Optimistic Voters` set consists of `> 2/3` of the stake, implies at least one optimistic validator `W` from the `Optimistic Voters` set must have submitted a vote (recall the definition of a switching proof),`Vote(X_w, S_w)` that was included in validator `V`'s switching proof for slot `X'`, where `S_w` contains a slot `s` such that:

- `s` não é um ancestral comum de `S.last` e `X'`
- `s` não é um descendente de `validator_vote.last`.
- `' + s'.lockout > S.last`

Como `B` é um ancestral de `S.last`, então também é verdade:

- `s` não é um ancestral comum de `S.last` e `X'`
- `' + s'.lockout > S.last`

que foi incluído na prova de mudança de `V`.

Agora porque `W` também é membro de `Votos Otimistas`, então pelo `Lemma 1` acima, dado um voto por `W`, `Voto(X_w, S_w)`, , onde `S_w` contém um voto para um slot `s` onde `s + s. ockout > B`, e `s` não é um ancestral de `B`, depois `X_w > B`.

Porque validador `V` incluído voto `Voto (X_w, S_w)` com a prova de mudar para o slot `X'`, então seu implica validador `V` submetido a votação `Vote(X_w, S_w)` **antes** validadores `V` enviaram sua votação por troca de slot `X'`, `Voto(X', S')`.

Mas isso é uma contradição porque escolhemos `Vote(X', S')` ser a primeira votação feita por qualquer validador nos `Optimiztic Voters` definem onde `X' > B` e `X'` é não é um descendente de `B`.
