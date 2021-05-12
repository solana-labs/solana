---
title: Pago simple y verificación del estado
---

A menudo es útil permitir a los clientes con pocos recursos participar en un clúster de Solana. Ya sea la ejecución económica o contractual, la verificación de que la actividad de un cliente ha sido aceptada por la red suele ser costosa. Esta propuesta establece un mecanismo para que dichos clientes confirmen que sus acciones se han comprometido con el estado del ledger con un gasto mínimo de recursos y confianza de terceros.

## Un planteamiento ingenuo

Los validadores almacenan las firmas de transacciones recientemente confirmadas por un corto período de tiempo para asegurarse de que no son procesadas más de una vez. Los validadores proporcionan un endpoint JSON RPC, que los clientes pueden usar para consultar al clúster si una transacción ha sido procesada recientemente. Los validadores también proporcionan una notificación de PubSub, por la que un cliente se registra para ser notificado cuando una firma determinada es observada por el validador. Aunque estos dos mecanismos permiten a un cliente verificar un pago, no son una prueba y dependen de que el validador confíe completamente en él.

Describiremos una forma de minimizar esta confianza utilizando Merkle Proofs para anclar la respuesta del validador en el ledger, permitiendo al cliente confirmar por sí mismo que un número suficiente de sus validadores preferidos han confirmado una transacción. Requerir múltiples atestados del validador reduce aún más la confianza en el validador, ya que aumenta la dificultad tanto técnica como económica de comprometer a varios otros participantes de la red.

## Clientes ligeros

Un 'cliente ligero' es un participante de cluster que no ejecuta un validador. Este cliente ligero proporcionaría un nivel de seguridad mayor que confiar en un validador remoto, sin requerir que el cliente ligero gaste muchos recursos verificando el ledger.

En lugar de proporcionar firmas de transacciones directamente a un cliente ligero, el validador genera en su lugar una prueba Merkle desde la transacción de interés hasta la raíz de un árbol Merkle de todas las transacciones en el bloque incluido. Esta Merkle Root se almacena en una entrada de registro que es votada por validadores, proporcionando legitimidad de consenso. El nivel de seguridad adicional para un cliente ligero depende de un conjunto canónico inicial de validadores que el cliente ligero considera que son las partes interesadas del clúster. A medida que este conjunto se cambia, el cliente puede actualizar su conjunto interno de validadores conocidos con [recibos](simple-payment-and-state-verification.md#receipts). Esto puede llegar a ser desafiante con un gran número de stake delegados.

Los propios validadores pueden querer usar APIs de clientes ligeros por razones de rendimiento. Por ejemplo, durante el lanzamiento inicial de un validador, éste puede utilizar un punto de control del estado proporcionado por el clúster y verificarlo con un recibo.

## Recibos

Un recibo es una prueba mínima de ello; una transacción ha sido incluida en un bloque, que el bloque ha sido votado por los validadores preferidos del cliente y que los votos han alcanzado la profundidad de confirmación deseada.

### Prueba de inclusión de transacción

Una prueba de inclusión de transacciones es una estructura de datos que contiene una ruta Merkle de una transacción, a través de un Entry-Merkle a un Block-Merkle, que se incluye en un Bank-Hash con el conjunto requerido de votos del validador. Una cadena de entradas PoH que contiene los votos subsiguientes de los validadores, derivados del Bank-Hash, es la prueba de la confirmación.

#### Transacción Merkle

Una Entry-Merkle es un Merkle Root que incluye todas las transacciones en una entrada dada, ordenadas por firma. Cada transacción en una entrada ya está merkled aquí: https://github.com/solana-labs/solana/blob/b6bfed64cb159ee67bb6bdbaefc7f833bbed3563/ledger/src/entry.rs#L205. Esto significa que podemos mostrar qie una transacción `T` fue incluida en una entrada `E`.

Un bloque-Merkle es el root Merkle de todos los Merkles secuenciados en el bloque.

![Diagrama de bloque Merkle](/img/spv-block-merkle.svg)

Juntas, las dos pruebas merkle muestran que una transacción`T` fue incluida en un bloque con el hash bancario `B`.

Un Accounts-Hash es el hash de la concatenación de los hashes de estado de cada cuenta modificada durante la ranura actual.

El estado de la transacción es necesario para el recibo porque el recibo de estado está construido para el bloque. Dos transacciones sobre el mismo estado pueden aparecer en el bloque, y por lo tanto, no hay manera de inferir a partir de sólo el estado si una transacción que se compromete con el ledger ha tenido éxito o fracasado en la modificación del estado previsto. Puede que no sea necesario codificar el código de estado completo, pero un solo bit de estado para indicar el éxito de la transacción.

Actualmente, el Block-Merkle no está implementado, por lo que para verificar que `E` era una entrada en el bloque con el hash del banco `B`, necesitaríamos proporcionar todos los hashes de las entradas en el bloque. Idealmente este Block-Merkle se implementaría, ya que la alternativa es muy ineficiente.

#### Cabeceras de bloque
Para verificar las pruebas de inclusión de la transacción, los clientes deben poder infectar la topología de los forks en la red

Más concretamente, el cliente ligero tendrá que hacer un seguimiento de las cabeceras de los bloques entrantes de manera que, dados dos hashes de bancos para los bloques `A` y `B`, puedan determinar si `A` es un ancestro de `B` (La sección siguiente sobre `Prueba de confirmación optimista` explica por qué). El contenido del encabezado son los campos necesarios para calcular el hash bancario.

Un Bank-Hash es el hash de la concatenación del Block-Merkle y el Accounts-Hash descritos en la sección `Transaction Merkle` anterior.

![Diagrama Bank-Hash](/img/spv-bank-hash.svg)

En el código:

https://github.com/solana-labs/solana/blob/b6bfed64cb159ee67bb6bdbaefc7f833bbed3563/runtime/src/bank.rs#L3468-L3473

```
        let mut hash = hashv(&[
            // bank hash of the parent block
            self.parent_hash.as_ref(),
            // hash of all the modifed accounts
            accounts_delta_hash.hash.as_ref(),
            // Number of signatures processed in this block
            &signature_count_buf,
            // Last PoH hash in this block
            self.last_blockhash().as_ref(),
        ]);
```

Un buen lugar para implementar esta lógica a lo largo de la lógica de streaming existente en la lógica de repetición del validador de: https://github.com/solana-labs/solana/blob/b6bfed64cb159ee67bb6bdbaefc7f833bbed3563/core/src/replay_stage.rs#L1092-L1096

#### Prueba de confirmación optimista

En la actualidad, la confirmación optimista se detecta a través de un oyente que supervisa los gossip y la pipeline de repetición para los votos: https://github.com/solana-labs/solana/blob/b6bfed64cb159ee67bb6bdbaefc7f833bbed3563/core/src/cluster_info_vote_listener.rs#L604-L614.

Cada voto es una transacción firmada que incluye el hash bancario del bloque por el que votó el validador, es decir, el `B` de la sección `Transacción Merkle` anterior. Una vez que un determinado umbral `T` de la red ha votado un bloque, éste se se considera óptimamente confirmado. Los votos realizados por este grupo de `T` validadores son necesarios para mostrar que el bloque con el hash del banco `B` fue optimistamente confirmado.

Sin embargo, aparte de algunos metadatos, los votos firmados en sí mismos no se almacenan actualmente en ninguna parte, por lo que no se pueden recuperar a petición. Estos votos probablemente necesitan ser persistidos en la base de datos Rocksdb, indexados por una clave `(Slot, Hash, Pubkey)` que representa la ranura del voto, el hash del banco del voto, y la pubkey de la cuenta del voto responsable del mismo.

Juntos, el merkle de la transacción y las pruebas de confirmación optimistas pueden proporcionarse a través de RPC a los suscriptores mediante la ampliación de la lógica de suscripción de firmas existente. Los clientes que se suscriben al nivel de confirmación "SingleGossip" ya son notificados cuando se detecta una confirmación optimista, se puede proporcionar una bandera para señalar que las dos pruebas anteriores también deben ser devueltas.

Es importante tener en cuenta que la confirmación optimista de `B` también implica que todos los bloques antecesores de `B` también están confirmados de forma optimista, y también que no todos los bloques serán confirmados de forma optimista.

```

B -> B'

```

Así, en el ejemplo anterior, si un bloque `B'` está confirmado de forma óptima, entonces también lo está `B`. Así, si una transacción estaba en el bloque `B`, el merkle de la transacción en la prueba será para el bloque `B`, pero los votos presentados en la prueba serán para el bloque `B'`. Por eso son importantes las cabeceras de la sección `Cabeceras de bloque`, el cliente tendrá que verificar que `B` es efectivamente un ancestro de `B'`.

#### Distribución de la prueba de stake

Una vez presentado el merkle de la transacción y las pruebas de confirmación optimista anteriores, un cliente puede verificar que una transacción `T` fue confirmada de forma optimista en un bloque con el hash bancario `B`. La última pieza que falta es cómo verificar que los votos en las pruebas optimistas anteriores constituyen realmente el porcentaje válido de `T` del stake necesario para mantener las garantías de seguridad de la "confirmación optimista".

Una forma de enfocar esto podría ser que en cada época, cuando el conjunto de stake cambie, se escriban todos los stakes en una cuenta del sistema, y luego que los validadores se suscriban a esa cuenta del sistema. Los nodos completos pueden entonces proporcionar un merkle probando que el estado de la cuenta del sistema fue actualizado en algún bloque `B`, y luego mostrar que el bloque `B` fue confirmado/rooteado de forma optimista.

### Verificación de estado de cuenta

El estado de una cuenta (balance u otros datos) puede ser verificado enviando una transacción con una **_TBD_** Instrucción al clúster. El cliente puede utilizar una [prueba de inclusión de transacciones](#transaction-inclusion-proof) para verificar si el clúster está de acuerdo en que el recuento ha alcanzado el estado esperado.

### Votos de validador

Los líderes deben unir los votos del validador por el peso del stake en una sola entrada. Esto reducirá el número de entradas necesarias para crear un recibo.

### Cadena de Entradas

Un recibo tiene un enlace PoH desde la raíz de la ruta Merkle Path del pago o del estado hasta una lista de votos de validación consecutivos.

Contiene lo siguiente:

- Transacción -&gt; Entry-Merkle -&gt; Block-Merkle -&gt; Bank-Hash

Y un vector de entradas PoH:

- Entradas de voto del validador
- Ticks
- Entradas ligeras

```text
/// Esta definición de entrada omite las transacciones y sólo contiene el hash
/// de las transacciones utilizadas para modificar PoH.
LightEntry {
    /// El número de hashes desde el ID de entrada anterior.
    pub num_hashes: u64,
    /// El hash `num_hashes` de SHA-256 después del ID de entrada anterior.
    hash: Hash,
    /// El Root Merkle de las transacciones codificadas en la Entrada.
    entry_hash: Hazh,
}
```

Las entradas ligeras se reconstruyen a partir de las entradas y simplemente muestran la entrada Merkle Root que se mezcló en el hash de PoH, en lugar de la transacción completa.

Los clientes no necesitan el estado de voto inicial. El algoritmo [ffork selection](../implemented-proposals/tower-bft.md) se define tal que sólo los votos que aparecen después de la transacción proporcionan la finalidad de la misma, y la finalidad es independiente del estado inicial.

### Verificación

Un cliente ligero que conoce a los validadores de súper mayoría puede verificar un recibo siguiendo la ruta de Merkle a la cadena PoH. El Block-Merkle es el Root de Merkle y aparecerá en votos incluidos en una Entrada. El cliente ligero puede simular [fork selection](../implemented-proposals/tower-bft.md) para los votos consecutivos y verificar que el recibo se confirma en el umbral de bloqueo deseado.

### Estado sintético

El estado sintético debe calcularse en el Banco-Hash junto con el estado generado por el banco.

Por ejemplo:

- Cuentas del validador de época y sus stake y pesos.
- Tasas calculadas

Estos valores deberían tener una entrada en el Banco-Hash. Deben vivir bajo cuentas conocidas, y por lo tanto tener un índice en la concatenación de hash.
