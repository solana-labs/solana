---
title: Replicación del Ledger
---

Nota: esta solución de replicación de ledger fue parcialmente implementada, pero no completada. La implementación parcial fue removida por https://github.com/solana-labs/solana/pull/9992 para prevenir el riesgo de seguridad de código no utilizado. La primera parte de este documento de diseño refleja las partes una vez implementadas de la replicación del ledger. La [segunda parte de este documento](#ledger-replication-not-implemented) describe las partes de la solución nunca implementadas.

## Prueba de Replicación

A plena capacidad en una red de 1gbps solana generará 4 petabytes de datos por año. Para evitar que la red se centralice alrededor de validadores que tienen que almacenar la totalidad de los datos, este protocolo propone una forma para que los nodos mineros proporcionen capacidad de almacenamiento para las piezas de los datos.

La idea básica de Prueba de Replicación es cifrar un conjunto de datos con una clave simétrica pública usando el cifrado CBC, luego cifrar el conjunto de datos cifrados. El principal problema con el enfoque ingenuo es que un nodo de almacenamiento deshonesto puede transmitir el cifrado y eliminar los datos a medida que se encripten. La solución simple es regenerar periódicamente el hash basado en un valor de PoH firmado. Esto asegura que todos los datos estén presentes durante la generación de la prueba y también requiere que los validadores tengan la totalidad de los datos cifrados presentes para la verificación de todas las pruebas de cada identidad. Así que el espacio necesario para validar es `number_of_proofs * data_size`

## Optimización con PoH

Nuestra mejora en este enfoque es la muestra aleatoria de los segmentos cifrados más rápido de lo que se necesita para cifrar, y registra el hash de esas muestras en el contador de PoH. Así, los segmentos permanecen en el mismo orden exacto para cada PoRep y la verificación puede transmitir los datos y verificar todas las pruebas en un solo lote. De esta manera podemos verificar múltiples pruebas simultáneamente, cada una en su propio núcleo CUDA. El espacio total requerido para la verificación es `1_ledger_segment + 2_cbc_blocks * number_of_identities` con número de núcleo igual a `number_of_identities`. Utilizamos un tamaño de bloque CBC de 64 bytes.

## Red

Validadores para PoRep son los mismos validadores que están verificando transacciones. Si un archivador puede probar que un validador verificó un PoRep, entonces el validador no recibirá una recompensa por ese epoch.

Los Archivadores son _clientes ligeros especializados_. Descargan una parte del ledger \(también conocido como segmento de la unidad\) y lo almacenan, y proporcionan PoReps de almacenar el ledger. Por cada archivero PoRep verificado gana una recompensa de sol de la piscina minera.

## Restricciones

Tenemos las siguientes restricciones:

- La verificación requiere generar los bloques CBC. Eso requiere un espacio de 2

  por identidad, y 1 núcleo CUDA por identidad para el mismo conjunto de datos. Así que como

  muchas identidades a la vez deberían ser combinadas con tantas pruebas para esas

  identidades verificadas simultáneamente para el mismo conjunto de datos.

- Los validadores mostrarán aleatoriamente el conjunto de pruebas de almacenamiento al conjunto que

  pueden manejar, y sólo los creadores de las pruebas elegidas serán

  recompensadas. El validador puede ejecutar una prueba de rendimiento cada vez que su configuración de hardware

  cambien para determinar qué tasa puede validar las pruebas de almacenaje.

## Protocolo de validación y replicación

### Constantes

1. SLOTS_PER_SEGMENT: Número de ranuras en un segmento de datos de ledger. La

   unidad de almacenamiento para un archivero.

2. NUM_KEY_ROTATION_SEGMENTS: Número de segmentos después de los cuales archiveros

   regenerar sus claves de cifrado y seleccionar un nuevo conjunto de datos para almacenar.

3. NUM_STORAGE_PROOFS: Número de pruebas de almacenamiento requeridas para una prueba de almacenamiento

   y reclamar ser recompensado con éxito.

4. RATIO_OF_FAKE_PROOFS: Ratio de pruebas falsas a pruebas reales que un almacenamiento

   debe contener para que la reclamación de prueba minera sea válida para una recompensa.

5. NUM_STORAGE_SAMPLES: Número de muestras requeridas para una minería de almacenamiento

   prueba.

6. NUM_CHACHA_ROUNDS: Número de rondas de cifrado realizadas para generar

   estado cifrado.

7. NUM_SLOTS_PER_TURN: Número de ranuras que definen un único epicentro de almacenamiento o

   un "turno" del juego PoRep.

### Comportamiento del validador

1. Los validadores se unen a la red y comienzan a buscar cuentas de archivador en cada uno de los

   límites de la época de almacenamiento/vuelta.

2. Cada turno, los validadores firman el valor de PoH en el límite y utilizan esa firma

   para seleccionar aleatoriamente pruebas para verificar de cada cuenta de almacenamiento encontrada en el límite de turno.

   Este valor firmado también se envía a la cuenta de almacenamiento del validador y será utilizado por

   los archivadores en una etapa posterior para verificarla.

3. Cada slot `NUM_SLOTS_PER_TURN` el validador anuncia el valor de PoH. Este es el valor

   se sirve también a Archivers a través de interfaces RPC.

4. Para un turno N determinado, todas las validaciones se bloquean hasta el turno N+3 (un intervalo de 2 turnos/epocas).

   En ese momento todas las validaciones durante ese turno están disponibles para la recolección de recompensas.

5. Cualquier validación incorrecta será marcada durante el turno intermedio.

### Comportamiento de archivador

1. Dado que un archivador es algo así como un cliente ligero y no descarga todos los

   datos del ledger tienen que depender de otros validadores y archiveros para obtener información.

   Cualquier validador puede o no ser malicioso y dar información incorrecta, aunque

   no hay ningún vector de ataque obvios que esto pueda conseguir además de tener el

   archivador hacer trabajo extra malgastado. Para muchas de las operaciones hay varias opciones

   dependiendo de lo paranoico que sea el archivero:

   - \(a\) archivador puede preguntar a un validador
   - \(b\) archivador puede preguntar a varios validadores
   - \(c\) archivador puede preguntar a otros archiveros
   - \(d\) archivador puede suscribirse al flujo completo de transacciones y generar

     la información misma \\(asumiendo que la ranura es lo suficientemente reciente\\)

   - \(e\) archivador puede suscribirse a un flujo de transacciones abreviado a

     generar la información en sí misma \(asumiendo que la ranura es lo suficientemente reciente\)

2. Un archivador obtiene el hash PoH correspondiente al último turno con su ranura.
3. El archivador firma el hash PoH con su keypair. Esa firma es la

   semilla usada para escoger el segmento para replicar y también la clave de cifrado. El

   archivador mods la firma con el slot para obtener a qué segmento

   replicar.

4. El archivador recupera el ledger preguntando a los validadores de los pares y

   archiveros. Ver 6.5.

5. El archivador entonces cifra ese segmento con la clave con el algoritmo chacha

   en modo CBC con `NUM_CHACHA_ROUNDS` de cifrado.

6. El archivador inicializa un rng de chacha con un valor reciente firmado como

   la semilla.

7. El archivador genera muestras `NUM_STORAGE_SAMPLES` en el rango del

   tamaño de la entrada y muestrea el segmento encriptado con sha256 para 32-bytes en cada

   valor de compensación. El muestreo del estado debe ser más rápido que la generación del cifrado

   segmento.

8. El archivador envía una transacción de prueba PoRep que contiene su estado sha

   al final de la operación de muestreo, su semilla y las muestras que solía

   el líder actual y se pone en el ledger.

9. Durante un turno dado el archivador debe presentar muchas pruebas para el mismo segmento

   y basado en el `RATIO_OF_FAKE_PROOFS` algunas de esas pruebas deben ser falsas.

10. A medida que el juego de PoRep entra en el siguiente turno, el archivador debe enviar un

    transacción con la máscara de que las pruebas eran falsas durante el último turno. Esta

    transacción definirá las recompensas tanto para los archiveros como para los validadores.

11. Finalmente para un turno N, mientras el juego PoRep entra en la ronda N + 3, pruebas del archivero durante

    el turno N se contabilizará para sus recompensas.

### El Juego PoRep

El juego Prueba de Replicación tiene 4 etapas primarias. Por cada "turno" múltiples juegos de PoRep pueden estar en progreso pero cada uno en una etapa diferente.

Las 4 etapas del juego PoRep son las siguientes:

1. Etapa de presentación de prueba
   - Archivadores: presentar tantas pruebas como sea posible durante esta etapa
   - Validadores: No-op
2. Fase de verificación de prueba
   - Archivadores: No-op
   - Validadores: Seleccione archiveros y verifique sus pruebas de la ronda anterior
3. Fase de prueba del desafío
   - Archivadores: Enviar la máscara de prueba con justificaciones \(para pruebas falsas presentadas hace 2 turnos\)
   - Validadores: No-op
4. Fase de colección de recompensas
   - Archivadores: Recoge recompensas por 3 turnos atrás
   - Validadores: Recoge recompensas por 3 turnos atrás

Por cada turno del juego PoRep, tanto Validadores como Archivers evalúan cada etapa. Las etapas se ejecutan como transacciones separadas en el programa de almacenamiento.

### Buscando quién tiene un bloque determinado de ledger

1. Los validadores supervisan los turnos en el juego PoRep y miran el banco arraigado

   a su vez los límites para cualquier prueba.

2. Los validadores mantienen un mapa de segmentos de ledger y claves públicas de archivador correspondientes.

   El mapa se actualiza cuando un Validador procesa las pruebas de un archivero para un segmento.

   El validador proporciona una interfaz RPC para acceder al mapa. Usando esta API, los clientes

   pueden mapear un segmento a la dirección de red de un archivero \(correlacionándolo a través de la tabla cluster_info\).

   Los clientes pueden enviar solicitudes de reparación al archivador para recuperar segmentos.

3. Los validadores necesitarían invalidar esta lista cada N turnos.

## Ataques Sybil

Para cualquier semilla aleatoria, obligamos a todos a utilizar una firma que se deriva de un hash PoH en el límite del turno. Todo el mundo usa el mismo contador, así que el mismo hash PoH está firmado por cada participante. A continuación, cada una de las firmas está vinculada criptográficamente al keypair, lo que impide que un líder pueda moler el valor resultante para más de una identidad.

Dado que hay muchas más identidades de clientes y luego identidades de cifrado, necesitamos dividir la recompensa para varios clientes, y evitar que los ataques Sybil generen muchos clientes para adquirir el mismo bloque de datos. Para seguir siendo BFT queremos evitar que una sola entidad humana almacene todas las réplicas de un solo trozo del ledger.

Nuestra solución es forzar a los clientes a seguir utilizando la misma identidad. Si la primera ronda se utiliza para adquirir el mismo bloque para muchas identidades de clientes, la segunda ronda para las mismas identidades del cliente forzará una redistribución de las firmas y, por lo tanto, las identidades y bloques de PoRep. Así para obtener una recompensa para los archiveros necesitan almacenar el primer bloque de forma gratuita y la red puede recompensar las identidades de clientes vividas durante mucho tiempo más que las nuevas.

## Ataques de validador

- Si un validador aprueba pruebas falsas, el archivador puede fácilmente sacarlas

  mostrando el estado inicial para el hash.

- Si un validador marca pruebas reales como falsas, no se puede hacer ningún cálculo en cadena

  para distinguir cual es correcto. Las recompensas deberían depender de los resultados de

  múltiples validadores para atrapar a los malos actores y evitar que se les nieguen las recompensas.

- Validador robando resultados de prueba de minería para sí mismo. Las pruebas se derivan

  de una firma de un archivero, ya que el validador no conoce la

  clave privada usada para generar la clave de cifrado, no puede ser el generador de

  la prueba.

## Incentivos de recompensa

Las pruebas falsas son fáciles de generar pero difíciles de verificar. Por esta razón, Las transacciones a prueba de PoRep generadas por archiveros pueden requerir una comisión más alta que una transacción normal para representar el costo computacional requerido por los validadores.

También es necesario cierto porcentaje de pruebas falsas para recibir una recompensa por la minería de almacenamiento.

## Notas

- Podemos reducir los costes de verificación de PoRep usando PoH, y de hecho

  hacer que sea factible verificar un gran número de pruebas para un conjunto de datos globales.

- Podemos eliminar la molienda obligando a todos a firmar el mismo hash PoH y

  usar las firmas como semilla

- El juego entre validadores y archivadores es sobre bloques aleatorios e

  identidades de cifrado aleatorias y muestras de datos aleatorias. El objetivo de la aleatorización es

  evitar que los grupos coludidos se superpongan en los datos o la validación.

- Los clientes del archivador pescan a los validadores perezosos presentando pruebas falsas que

  pueden demostrar que son falsas.

- Para defenderse de las identidades de clientes Sybil que intentan almacenar el mismo bloque

  obligamos a los clientes a almacenar durante varias rondas antes de recibir una recompensa.

- Los validadores también deberían ser recompensados por validar las pruebas de almacenamiento

  enviadas como incentivo para almacenar el ledger. Sólo pueden validar las pruebas si

  almacenan esa porción del ledger.

# Replicación de Ledger no implementada

Comportamiento de replicación aún por implementar.

## Época de almacenamiento

La época de almacenamiento debe ser el número de ranuras que resulta en alrededor de 100GB-1TB de ledger que se generará para los archivadores para almacenar. Los archivadores comenzarán a almacenar el ledgerr cuando un fork determinado tenga una alta probabilidad de no ser revertida.

## Comportamiento del validador

1. Cada NUM_KEY_ROTATION_TICKS también valida las muestras recibidas de

   archivadores. Firma el hash PoH en ese punto y utiliza el siguiente

   algoritmo con la firma como entrada:

   - Los 5 bits bajos del primer byte de la firma crean un índice en

     otro byte inicial de la firma.

   - El validador mira el conjunto de pruebas de almacenamiento donde el byte de

     el vector de estado sha de prueba a partir del byte bajo coincide exactamente

     con el byte\(s\) seleccionado de la firma.

   - Si el conjunto de pruebas es mayor que el validador puede manejar, entonces

     aumenta hasta que coincida con 2 bytes en la firma.

   - El validador sigue aumentando el número de bytes coincidentes hasta que se encuentra un

     conjunto viable.

   - Luego crea una máscara de pruebas válidas y pruebas falsas y la envía a

     el líder. Esta es una transacción de confirmación de prueba de almacenamiento.

2. Después de un periodo de bloqueo de NUM_SECONDS_STORAGE_LOCKOUT segundos, el

   el validador presenta entonces una transacción de reclamación de prueba de almacenamiento que hace que el

   distribución de la recompensa de almacenamiento si no se han visto retos para la prueba a los

   validadores y archivadores parte de las pruebas.

## Comportamiento de archivador

1. A continuación, el archivador genera otro conjunto de compensaciones que presenta una prueba falsa

   con un estado sha incorrecto. Se puede demostrar que es falsa proporcionando

   la semilla del resultado hash.

   - Una prueba falsa debe consistir en un hash del archivador de una firma de un

     valor PoH. De esta manera, cuando el archivador revele la prueba falsa, ésta podrá ser

     verificada en cadena.

2. El archivero supervisa el ledgerr, si ve una prueba falsa integrada, crea

   una transacción de desafío y la presenta al líder actual. La

   transacción demuestra que el validador validó incorrectamente una prueba de almacenamiento falsa.

   El archivero es recompensado y el saldo del staking del validador es recortado o

   congelado.

## Lógica de contrato de prueba de almacenamiento

Cada archivador y validador tendrán su propia cuenta de almacenamiento. La cuenta del validador estaría separada de su id de gossip similar a su cuenta de voto. Estos deberían ser implementados como dos programas uno que maneja al validador como el firmante de claves y otro para el archivero. De esa manera cuando los programas hacen referencia a otras cuentas, pueden comprobar el id del programa para asegurarse de que es una cuenta de validador o archivador a la que hacen referencia.

### Enviar prueba de minería

```text
SubmitMiningProof {
    slot: u64,
    sha_state: Hash,
    signature: Signature,
};
keys = [archiver_keypair]
```

Los Archivers crean estos datos después de minar sus datos contables almacenados para un determinado valor de hash. La ranura es la ranura final del segmento del ledger que están almacenando, el sha_state el resultado del archivador usando la función hash para muestrear su segmento de ledger cifrado. La firma es la que se creó cuando se firmó un valor PoH para la época de almacenamiento actual. La lista de pruebas de la época actual de almacenamiento debe guardarse en el estado de la cuenta, y luego transferirse a una lista de pruebas de la época anterior cuando ésta pase. En una determinada época de almacenamiento, un determinado archivero sólo debe presentar pruebas para un segmento.

El programa debe tener una lista de ranuras que son ranuras de minería de almacenamiento válidas. Esta lista debe ser mantenida manteniendo un seguimiento de las ranuras que están enraizadas en las que una parte significativa de la red ha votado con un alto valor de bloqueo. tal vez 32 votos de viejo. Cada número de ranuras SLOTS_PER_SEGMENT será añadido a este conjunto. El programa debería comprobar que la ranura está en este conjunto. El conjunto se puede mantener recibiendo un AdvertiseStorageRecentBlockHash y comprobando el estado de su banco/Tower BFT.

El programa debe verificar la firma de la firmante, clave pública desde el envío de la transacción y el mensaje del valor de poH de almacenamiento anterior.

### Validación

```text
ProofValidation {
   proof_mask: Vec<ProofStatus>,
}
keys = [validator_keypair, archiver_keypair(s) (unsigned)]
```

Un validador enviará esta transacción para indicar que un conjunto de pruebas para un segmento determinado son válidas/no válidas o se omite donde el validador no lo mire. Los keypairs de los archivadores que miró deben ser referenciados en las claves para que la lógica del programa pueda ir a esas cuentas y ver que las pruebas se generan en la época anterior. El muestreo de las pruebas de almacenamiento debe verificarse asegurando que el validador omita las pruebas correctas de acuerdo con la lógica expuesta en el comportamiento de muestreo del validador.

Las claves del archivador incluidas indicarán las muestras de almacenamiento a las que se hace referencia; la longitud de la proof_mask debe verificarse con el conjunto de pruebas de almacenamiento en la cuenta(s) del archivador referenciado, y debe coincidir con el número de pruebas presentadas en la época de almacenamiento anterior en el estado de dicha cuenta del archivador.

### ClaimStorageReward

```text
ClaimStorageReward {
}
keys = [validator_keypair or archiver_keypair, validator/archiver_keypairs (unsigned)]
```

Los archivadores y validadores utilizarán esta transacción para obtener tokens pagados de un estado del programa en el que SubmitStorageProof, ProofValidation y ChallengeProofValidations están en un estado en el que las pruebas han sido presentadas y validadas y no hay ChallengeProofValidations que hagan referencia a esas pruebas. En el caso de un validador, debe hacer referencia a los keypairs del archivador para los que ha validado pruebas en la época correspondiente. Y para un archivador debe hacer referencia a los keypairs del validador para los que ha validado y quiere ser recompensado.

### Validación a prueba de desafío

```text
ChallengeProofValidation {
    proof_index: u64,
    hash_seed_value: Vec<u8>,
}
keys = [archiver_keypair, validator_keypair]
```

Esta transacción es para atrapar validadores perezosos que no están haciendo el trabajo para validar pruebas. Un archivador enviará esta transacción cuando vea que un validador ha aprobado una transacción falsa de SubmitMiningProof. Dado que el archivador es un cliente ligero que no mira la cadena completa, tendrá que pedir esta información a un validador o a un conjunto de validadores, tal vez mediante una llamada RPC para obtener todas las ProofValidations de un determinado segmento en la época de almacenamiento anterior. El programa buscará en el estado de la cuenta del validador ver que un ProofValidation es enviado en la época de almacenamiento anterior y hash el hash_seed_value y ver que el hash coincide con la transacción SubmitMiningProof y que el validador lo marcó como válido. De ser así, entonces salvará el desafío a la lista de desafíos que tiene en su estado.

### AdvertiseStorageRecentBlockhash

```text
AdvertiseStorageRecentBlockhash {
    hash: Hash,
    slot: u64,
}
```

Los validadores y archiveros presentarán esto para indicar que ha pasado una nueva época de almacenamiento y que las pruebas de almacenamiento que son actuales deben ser ahora para la época anterior. Las demás transacciones deben comprobar que la época a la que hacen referencia es exacta según el estado actual de la cadena.
