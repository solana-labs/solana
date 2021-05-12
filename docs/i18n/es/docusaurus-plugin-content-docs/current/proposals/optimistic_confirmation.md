---
title: Confirmación optimista
---

## Primitivos

`vote(X, S)` - Los votos se aumentarán con una ranura de "referencia", `X` que es el **latest** de este fork fue este validador votó con una prueba de cambio. Mientras el validador realice votaciones consecutivas que desciendan unas de otras, se debe utilizar el mismo `X` para todas esas votaciones. Cuando el validador realiza una votación para una ranura `s` que no desciende de la anterior, `X` se establecerá en la nueva ranura `s`. Todos los votos entonces serán de la forma `voto(X, S)`, donde `S` es la lista ordenada de ranuras `(s, s. ockout)` votando a favor.

Dado un voto `vote(X, S)`, deja `S.last == vote.last` ser la última ranura en `S`.

Ahora definimos algunas condiciones de corte "Optimistic Slashing". La intuición para estos se describe a continuación:

- `Intuición`: Si un validador envía `vote(X, S)`, el mismo validador no debería haber votado en un fork diferente que "superpone" a este fork. Más concretamente, este validador no debería haber emitido otro voto `vote(X', S')` donde el rango `[X, S. última]` superpone el rango `[X', S'.last]`, `X != X'`, como se muestra a continuación:

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

(Ejemplo de votos barajables (X', S') y voto(X, S))

En el diagrama anterior, ten en cuenta que la votación a favor de `S. el pasado` debe haber sido enviado después de el voto por `S". pasados` (debido a bloqueos, el voto más alto debe haber sido enviado más tarde). Así, la secuencia de votos debe haber sido: `X ... S'.last ... S.last`. Esto significa después de la votación sobre `S". ast`, el validador debe haber cambiado de vueltan al otro fork en alguna ranura `s > S. el último > X`. Por lo tanto, la votación a favor de `S. el pasado` debería haber usado `s` como punto de "referencia" no `X`, porque ese fue el último "switch" en el fork.

Para hacer cumplir esto, definimos las condiciones de corte "Optimistic Slashing". Dado dos votos distintos `voto(X, S)`y `voto(X', S')` por el mismo validador, los votos deben satisfacer:

- `X <= S.last`, `X' <= S'.last`
- Todos los `s` en `S` son ancestros/descendientes unos de otros, todos los `s` en `S` son ancestros/descendientes unos de otros,
-
- `X == X'` implica `S` es padre de `S'` o `S'` es padre de `S`
- `X' > X` implica `X' > S. last` y `S'.last > S. last` y para todos `s` en `S`, `s + bloqueo(s) < X'`
- `X' > X` implica `X' > S. last` y `S'.last > S. last` y para todos `s` en `S`, `s + bloqueo(s) < X'`

(Las dos últimas reglas implican que los rangos no se pueden solapar): De lo contrario, el validador es recortado.

`Range(vote)` - Dado un voto `v = voto(X, S)`, definir `Range(v)` para ser el rango de ranuras `[X, S. último]`.

`SP(old_vote, new_vote)` Esta es la "prueba de cambio" para `old_vote`, el último voto del validador. Tal prueba es necesaria en cualquier momento en que un validador cambie su franja horaria de "referencia" (vea la sección de voto anterior). El comprobante de conmutación incluye una referencia a `Old_vote`, para que haya un registro de lo que era el "rango" de ese `old_vote` (para hacer otros interruptores conflictivos en este rango de barreras). Un cambio de este tipo debe seguir respetando los bloqueos.

Un comprobante de cambio muestra que `> 1/3` de la red está bloqueado en la ranura `old_vote.last`.

La prueba es una lista de elementos `(validator_id, validator_vote(X, S))`, donde:

1. La suma del stake de todos los Id del validador `> 1/3`

2. Para cada `(validator_id, validator_vote(X, S))`, existe alguna ranura `s` en `S` donde: _ a.`s` no es un ancestro común de ambos `validator_vote.last` y `antiguo_voto.last` y `nuevo_voto.last`. _ b. `s` no es un descendiente de `validator_vote.last`. \* c. `s + s.lockout() >= old_vote. ast` (implica que el validador aún está bloqueado en la ranura `s` en la ranura `old_vote.last`).

El cambio de forks sin un comprobante de cambio válido es cortable.

## Definiciones:

Confirmación optimista - Se dice que un bloque `B` ha alcanzado "confirmación optimista" si `>2/3` del stake han votado con votos `v` donde `Range(v)` para cada uno `v` incluye `B. mucho`.

Finalizado - Se dice que un bloque `B` está finalizado si al menos un validador correcto ha rootado `B` o un descendiente de `B`.

Estimado - Se dice que se revierte un bloque `B` si otro bloque `B` que no es un padre o descendiente de `B` fue finalizado.

## Garantías:

Un bloque `B` que ha alcanzado una confirmación optimista no será revertido a menos que se haya barrido al menos un validador.

## Prueba:

Supongamos por contradicción, un bloque `B` ha alcanzado `confirmación optimista` en alguna ranura `B + n` para algunos `n`, y:

- Otro bloque `B'` que no es padre o descendiente de `B` fue finalizado.
- Ningún validador violó ninguna condición de slashing.

Por la definición de `confirmación optimista`, esto significa `> 2/3` de validadores han mostrado un voto `v` del formulario `Vote(X, S)` donde `X <= B <= v. pasado`. Llamar a este conjunto de validadores los `Validadores optimistas`.

Ahora se ha dado un validador `v` en `Validadores Optimistas`, dos votos realizados por `-`, `Vote(X, S)` y `Vote(X', S')` donde `X <= B <= S. el last`, y `X' <= B <= S'. last`, then `X == X'` de lo contrario una condición de "slashing Optimista" es violada (los "rangos" de cada voto se superpondrían `B`).

Así definimos los `Votos Optimistas` para ser el conjunto de votos hechos por `Validadores Optimistas`, donde para cada validador optimista `v`, el voto hecho por `v` incluido en el conjunto es el `máximo` voto `Vote(X, S)` con el más grande `S.last` de cualquier voto realizado por `v` que satisfacen `X <= B <= S.last`. Como sabemos por lo anterior que `X` para todos los votos realizados por `v` es único, sabemos que hay un voto `máximo` único.

### Lemma 1:

`Reclamar:` Se le ha dado un voto `Vote(X, S)` hecho por un validador `V` en el conjunto `Validadores Optimistas` y `S` contiene un voto para una ranura `s` para que:

- `s + s.lockout > B`,
- `s` no es un antepasado o descendiente de `B`,

entonces `X > B`.

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

`Prueba`: Supongamos por contradicción que un validador `V` del conjunto "Validadores Optimistas" hizo tal votación `Vote(X, S)` donde `S` contiene un voto para una ranura `s` no es un antepasado o descendiente de `B`, donde `s + s. ockout > B`, pero `X <= B`.

Dejar `Vote(X', S')` ser el voto en `Votos Optimistas` establecidos por el validador `V`. Por definición de ese conjunto (todos los votos confirmados optimistamente `B`), `X' <= B <= S'.last` (ver el diagrama anterior).

Esto implica que porque se asume por encima de `X <= B`, entonces `X <= S'.last`, así por las reglas de slashing, `X == X'` o `X < X'` (de lo contrario solaparía el rango `(X', S'.last)`).

`Caso X == X'`:

Considere `s`. ¡Conocemos `s X` porque se asume que `s` no es un ancestro o descendiente de `B`, y `X` es un antepasado de `B`. Porque `S'.last` es un descendiente de `B`, esto significa que `s` tampoco es un antepasado o descendiente de `S'. último`. Luego, debido a que `S.last` es descendido de `s`y luego `S.last` tampoco puede ser un antepasado o descendiente de `S.last`. Esto implica `X != X'` por las reglas "Optimistic Slashing".

`Caso X < X'`:

Intuitivamente, esto implica que `Vote(X, S)` se hizo "antes" `Vote(X', S')`.

De la suposición anterior, `s + s.lockout > B > X'`. Como `s` no es un ancestro de `X'`, los bloqueos se habrían violado cuando este validador intentó por primera vez enviar un voto de cambio a `X'` con algún voto de la forma `Voto(X', S'')`.

Puesto que ninguno de estos casos son válidos, el supuesto debe haber sido inválido, y el reclamo está probado.

### Lemma 2:

Recuerda `B'` fue el bloque finalizado en un fork diferente que bloque "optimistamente" confirmado `B`.

`Reclama`: Para cualquier voto `Vote(X, S)` en el conjunto `Votos Opimistas` debe ser cierto que `B' > X`

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

`Proof`: Let `Vote(X, S)` be a vote in the `Optimistic Votes` set. Luego, por definición, dado el bloque "optimistamente" `B`, `X <= B <= S.last`.

Porque `X` es un padre de `B`, y `B'` no es un padre o ancestro de `B`, entonces:

- `B' != X`
- `B'` no es padre de `X`

Ahora considere si `B'` < `X`:

`Caso B' < X`: Vamos a mostrar que esto es una violación de bloqueos. Desde arriba, sabemos que `B` no es un padre de `X`. Entonces porque `B'` estaba arraigado, y `B'` no es padre de `X`, entonces el validador no debería haber podido votar en la ranura más alta `X` que no desciende de `B'`.

### Prueba de seguridad:

Ahora apuntamos a mostrar al menos uno de los validadores en el conjunto de `Validadores Optimos` violó una regla de slashing.

Primera nota de que para que `B` haya sido rooteado, debe haber habido `> 2/3` stake que votó en `B` o un descendiente de `B`. Dado que el conjunto de `Validador Opítimista` también contiene `> 2/3` de los validadores con stake, se deduce que `> 1/3` de los validadores con stake:

- `B`Rooteado o un descendiente de `B`
- También envió un voto `v` del formulario `Vote(X, S)` donde `X <= B <= v.last`.

Deja que el `Delincuente` sea el conjunto de validadores que cumplan con los criterios anteriores.

Por definición, para rootear `B'`, cada validador `V` en `Delincuente` debe haber hecho algún "voto de cambio" del formulario `Vote(X_v, S_v)` dónde:

- `S_v.last > B'`
- `S_v.last` es un descendiente de `B`, por lo que no puede ser un descendiente de `B`
- Porque `S_v. last` no es un descendiente de `B`, entonces `X_v` no puede ser un descendiente o antepasado de `B`.

Por definición, este validador `V` también votó `Vote(X, S)` en los `Votos Optimistas` donde por definición de ese conjunto (optimistamente confirmó `B`), conocemos `S. last >= B >= X`.

Por `Lemma 2` conocemos que `B' > X`, y por encima `S_v.last > B'`, entonces `S_v.last > X`. Porque `X_v ! X` (no puede ser un descendiente o antepasado de `B` desde arriba), luego por las reglas de slashing entonces, sabemos que `X_v > S.last`. Desde arriba, `S.last >= B >= X` así que para todos los "cambios de votos", `X_v > B`.

Ahora ordenando todos estos "cambios de votos" a tiempo, deja a `V` para ser el validador en `Validadores Optimistas` que enviaron por primera vez un "cambio de votos" `Vote(X', S')`, donde `X' > B`. Sabemos que ese validador existe porque sabemos por lo anterior que todos los validadores morosos deben haber presentado ese voto, y los validadores morosos son un subconjunto de los `Validadores optimistas`.

Dejar `voto(X, S)` ser el voto único en `Votos Optimistas` realizados por validador `V` (maximizando `S.last`).

Dado `Vote(X, S)` porque `X' > B >= X`, entonces `X' > X`, así que por las reglas "Optimistic Slashing", `X' > S.last`.

Para realizar este "voto de cambio" a `X'`, una prueba de cambio `SP(Voto(X, S), Voto(X', S'))` debe mostrar que `> 1/3` de la participación está bloqueada en el último voto de este validador, `S.last`. Combine este `>1/3` con el hecho de que el conjunto de validadores en el conjunto de `Votantes Optimistas` consiste en `> 2/3` del stake, implica al menos un validador optimista `W` del conjunto de `votantes optimistas` debe haber emitido un voto (recuérdese la definición de una prueba de cambio), `Voto(X_w, S_w)` que se incluyó en la prueba de cambio del validador `V` prueba de cambio para la ranura `X'`, donde `S_w` contiene una ranura `s` tal que:

- `s` no es un antepasado común de `S.last` y `X'`
- `s` no es un descendiente de `S.last`.
- `s' + s'.lockout > S.last`

Porque `B` es un antepasado de `S.last`, entonces también es cierto:

- `s` no es un antepasado común de `B` y `X`
- `s' + s'.lockout > B`

que fue incluido en la prueba de cambio de `V`.

Ahora bien, como `W` es también un miembro de `Votantes optimistas`, entonces por el `Lema 1` anterior, dado un voto de `W`, `Vote(X_w, S_w)`, donde `S_w` contiene un voto para una ranura `s` donde `s + s. lockout > B`, y `s` no es un ancestro de `B`, entonces `X_w > B`.

Porque el validador `V` incluyó el voto `Vote(X_w, S_w)` en su prueba de cambio para la ranura `X'`, entonces su implica que el validador `V'` presentó el voto `Vote(X_w, S_w)` **antes** de que el validador `V` presentara su voto de conmutación para la ranura `X'`, `Voto(X', S')`.

Pero esto es una contradicción porque elegimos `Vote(X', S')` para ser el primer voto realizado por cualquier validador en el `Votantes optimistas` establecido donde `X' > B` y `X'` es no un descendiente de `B`.
