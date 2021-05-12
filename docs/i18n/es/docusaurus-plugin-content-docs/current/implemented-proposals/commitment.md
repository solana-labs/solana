---
title: Compromiso
---

La métrica de compromiso apunta a dar a los clientes una medida de la confirmación de la red y niveles de apuesta en un bloque en particular. Los clientes pueden entonces usar esta información para derivar sus propias medidas de compromiso.

# Calcular RPC

Los clientes pueden solicitar métricas de compromiso de un validador para una firma `s` a través de `get_block_commitment(s: Signature) -> BlockCommitment` sobre RPC. La estructura `BlockCommitment` contiene un array de u64 `[u64, MAX_CONFIRMATIONS]`. Este arreglo representa la métrica de compromiso para el bloque `N` en particular que contiene la firma `s` al último bloque `M` en el que votó el validador.

Una entrada `s` en índice `i` en el array `BlockCommitment` implica que el validador observó `s` la participación total en el clúster llegando a `i` confirmaciones en bloque `N` como se observó en algún bloque `M`. Habrá `MAX_CONFIRMMATIONS` elementos en este arreglo, representando todo el posible número de confirmaciones de 1 a `MAX_CONFIRMATIONS`.

# Cálculo de la métrica de compromiso

Construyendo este `BlockCommitment` struct aprovecha los cálculos que ya están siendo realizados para construir consenso. La función `collect_vote_lockouts` en `consenso. s` construye un HashMap, donde cada entrada es de la forma `(b, s)` donde `s` es la cantidad de participación en un banco `b`.

Este cálculo se realiza en un banco candidato votable `b` de la siguiente manera.

```text
   let output: HashMap<b, Stake> = HashMap::new();
   for vote_account in b.vote_accounts {
       for v in vote_account. vote_stack {
           for a in ancestors(v) {
               f(*output. et_mut(a), vote_account, v);
           }
       }
}
```

donde `f` es una función de acumulación que modifica la entrada `Stake` para la ranura `a` con algunos datos derivables de vote `v` y `vote_account` (apuesta, bloqueo, etc.). Ten en cuenta que los `ancestors` aquí solo incluyen ranuras presentes en la caché de estado actual. Las firmas para los bancos antes de lo que los presentes en la caché de estado no serían consultables de todos modos, por lo que esos bancos no están incluidos en los cálculos de compromiso aquí.

Ahora naturalmente podemos aumentar el cálculo anterior para crear también un arreglo de `BlockCommitment` para cada banco `b` por:

1. Añadir un `ForkCommitmentCache` para recoger las estructuras `BlockCommitment`
2. Reemplazando `f` por `f'` de tal manera que el cálculo anterior también construye este `BlockCommitment` por cada banco `b`.

Procederemos con los detalles de 2) ya que 1) es trivial.

Antes de continuar, cabe destacar que para la cuenta de voto de algún validador `a`, el número de confirmaciones locales para ese validador en la ranura `s` es `v.num_confirmations`, donde `v` es el voto más pequeño en la pila de votos `a.votes` tal que `v.slot >= s` (es decir, no hay necesidad de mirar votos > v ya que el número de confirmaciones será menor).

Ahora más específicamente, aumentamos el cálculo anterior a:

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

donde `f'` está definido como:

```text
    fn f`(
        stake: &mut Stake,
        some_ancestor: &mut BlockCommitment,
        vote_accountt: VoteAccount,
        v: Vote, total_stake: u64
    ){
        f(stake, vote_account, v);
        *some_ancestor.commitment[v.num_confirmations] += vote_account.stake;
}
```
