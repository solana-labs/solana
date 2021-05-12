---
title: Rotación del líder
---

En un momento dado, un clúster espera que solo un validador produzca entradas en el libro mayor. Al tener solo un líder a la vez, todos los validadores pueden reproducir copias idénticas del libro mayor. Sin embargo, el inconveniente de un solo líder a la vez es que un líder malintencionado es capaz de censurar votos y transacciones. Dado que la censura no se puede distinguir de la red que descarta paquetes, el clúster no puede simplemente elegir un solo nodo para que mantenga el rol de líder de manera indefinida. En cambio, el clúster minimiza la influencia de un líder malintencionado al rotar qué nodo toma la iniciativa.

Cada validador selecciona el líder esperado utilizando el mismo algoritmo, que se describe a continuación. Cuando el validador recibe una nueva entrada firmada del libro mayor, puede estar seguro de que el líder esperado produjo una entrada. El orden de los espacios en los que se asigna un espacio a cada líder se denomina _ programa de líderes _.

## Rotación del Horario del Líder

Un validador rechaza los bloques que no están firmados por el _ líder de la ranura _. La lista de identidades de todos los líderes de ranuras se denomina _ programa de líderes _. El cronograma del líder se vuelve a calcular localmente y periódicamente. Asigna líderes de espacio durante un período de tiempo llamado _ época _. El cronograma debe calcularse mucho antes de los espacios que asigna, de modo que se finalice el estado del libro mayor que utiliza para calcular el cronograma. Esa duración se denomina _ compensación de la programación del líder _. Solana establece el desplazamiento a la duración de los espacios hasta la próxima época. Es decir, el cronograma líder para una época se calcula a partir del estado del libro mayor al comienzo de la época anterior. El desplazamiento de una época es bastante arbitrario y se supone que es lo suficientemente largo como para que todos los validadores hayan finalizado su estado de libro mayor antes de que se genere la siguiente programación. Un clúster puede optar por acortar la compensación para reducir el tiempo entre los cambios de participación y las actualizaciones del cronograma del líder.

Mientras opera sin particiones que duren más de una época, el cronograma solo necesita generarse cuando la bifurcación de la raíz cruza el límite de la época. Dado que el cronograma es para la próxima época, las nuevas apuestas comprometidas con la bifurcación raíz no estarán activas hasta la próxima época. El bloque utilizado para generar el programa líder es el primer bloque que cruza el límite de la época.

Sin una partición que dure más de una época, el clúster funcionará de la siguiente manera:

1. Un validador actualiza continuamente su propia bifurcación raíz a medida que vota.
2. El validador actualiza su cronograma líder cada vez que la altura de la ranura cruza un límite de época.

Por ejemplo:

La duración de la época es de 100 slots. La bifurcación raíz se actualiza desde la bifurcación calculada a la altura de la ranura 99 a una bifurcación calculada a la altura de la ranura 102. Las horquillas con ranuras a la altura 100, 101 se omitieron debido a fallas. El nuevo programa de líder se calcula utilizando una bifurcación a la altura de la ranura 102. Está activo desde la ranura 200 hasta que se actualiza nuevamente.

No puede existir ninguna inconsistencia porque cada validador que está votando con el clúster ha omitido 100 y 101 cuando su raíz pasa 102. Todos los validadores, independientemente del patrón de votación, se comprometerían con una raíz que sea 102 o un descendiente de 102.

### Rotación de cronograma de líder con particiones de tamaño de época.

La duración de la compensación del cronograma líder tiene una relación directa con la probabilidad de que un grupo tenga una visión inconsistente del cronograma líder correcto.

Consideremos el siguiente escenario:

Dos particiones que están generando la mitad de los bloques cada una. Ninguno de los dos está llegando a una bifurcación de supermayoría definitiva. Ambos cruzarán la época 100 y 200 sin comprometerse realmente con una raíz y, por lo tanto, un compromiso de todo el clúster con un nuevo programa líder.

En este escenario inestable, existen múltiples programas de líderes válidos.

- Se genera un cronograma líder para cada bifurcación cuyo padre directo está en la época anterior.
- El programa líder es válido después del inicio de la siguiente época para las bifurcaciones descendientes hasta que se actualice.

El horario de cada partición divergerá después de que la partición dure más de una época. Por esta razón, la duración de la época debe seleccionarse para que sea mucho mayor que el tiempo de la ranura y la longitud esperada para que una bifurcación se comprometa con la raíz.

Después de observar el conglomerado durante un período de tiempo suficiente, se puede seleccionar el desplazamiento del programa líder en función de la duración de la partición media y su desviación estándar. Por ejemplo, una compensación mayor que la duración media de la partición más seis desviaciones estándar reduciría la probabilidad de una programación del libro mayor inconsistente en el clúster a 1 en 1 millón.

## Generación de cronogramas de líderes en Genesis

La configuración de génesis declara el primer líder de la primera época. Este líder termina programado para las dos primeras épocas porque el horario del líder también se genera en el espacio 0 para la siguiente época. La longitud de las dos primeras épocas también se puede especificar en la configuración de génesis. La duración mínima de las primeras épocas debe ser mayor o igual que la profundidad de retroceso máxima definida en [ Tower BFT ](../implemented-proposals/tower-bft.md).

## Algoritmo de generación de programa de líder

El cronograma del líder se genera utilizando una semilla predefinida. El proceso es el siguiente:

1. Utilice periódicamente la altura de tick PoH \ (un contador que aumenta monótonamente \) para generar un algoritmo pseudoaleatorio estable.
2. A esa altura, muestree en el banco todas las cuentas apostadas con identidades de líder que han votado dentro de un número de ticks configurado en grupo. La muestra se denomina _ conjunto activo _.
3. Ordene el conjunto activo por peso del Stake.
4. Utilice la semilla aleatoria para seleccionar nodos ponderados por participación para crear un orden ponderado por participación.
5. Este orden se vuelve válido después de un número de ticks configurado en clúster.

## Programar vectores de ataque

### Semilla

La semilla que se selecciona es predecible pero insuperable. No hay ningún ataque contundente que influya en su resultado.

### Conjunto activo

Un líder puede sesgar el conjunto activo censurando los votos de los validadores. Existen dos formas posibles para que los líderes censuren el conjunto activo:

- Ignorar los votos de los validadores
- Negarse a votar por bloques con votos de validadores

Para reducir la probabilidad de censura, el conjunto activo se calcula en el límite de compensación de la programación líder sobre una _ duración de muestreo del conjunto activo _. La duración del muestreo del conjunto activo es lo suficientemente larga como para que varios líderes hayan recopilado los votos.

### Staking

Los líderes pueden censurar nuevas transacciones de participación o negarse a validar bloques con nuevas participaciones. Este ataque es similar a la censura de los votos de los validadores.

### Pérdida de la clave operativa del validador

Se espera que los líderes y validadores utilicen claves efímeras para la operación, y los propietarios de las estacas autorizan a los validadores a trabajar con su participación a través de la delegación.

El clúster debería poder recuperarse de la pérdida de todas las claves efímeras utilizadas por líderes y validadores, lo que podría ocurrir a través de una vulnerabilidad de software común compartida por todos los nodos. Los propietarios de Stake deben poder votar directamente firmando conjuntamente un voto de validador, aunque la participación esté actualmente delegada a un validador.

## Anexar entradas

La duración de la programación de un líder se denomina _ época _. La época se divide en _ ranuras _, donde cada ranura tiene una duración de ` T ` PoH ticks.

Un líder transmite entradas durante su espacio. Después de las marcas de ` T `, todos los validadores cambian al siguiente líder programado. Los validadores deben ignorar las entradas enviadas fuera del espacio asignado a un líder.

Todos los ticks de ` T ` deben ser observados por el siguiente líder para que pueda construir sus propias entradas. Si las entradas no se observan \ (el líder está abajo \) o las entradas no son válidas \ (el líder tiene errores o es malicioso \), el siguiente líder debe producir ticks para llenar el espacio del líder anterior. Tenga en cuenta que el próximo líder debe realizar las solicitudes de reparación en paralelo y posponer el envío de ticks hasta que esté seguro de que otros validadores tampoco observaron las entradas del líder anterior. Si un líder construye incorrectamente sus propios ticks, el líder que lo sigue debe reemplazar todos sus ticks.
