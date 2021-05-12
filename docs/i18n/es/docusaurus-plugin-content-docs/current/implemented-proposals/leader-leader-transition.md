---
title: Transición de líder a líder
---

Este diseño describe cómo los líderes de la transición de producción del libro de ganancias PoH entre sí a medida que cada líder genera su propia ranura.

## Desafíos

El líder actual y el próximo líder están en carrera para generar la marca final de la rama actual. El próximo líder puede llegar a esa franja horaria mientras sigue procesando las entradas del líder actual.

El escenario ideal sería que el próximo líder generara su propia franja horaria justo después de que pudiera votar por el líder actual. Es muy probable que el próximo líder llegue a su altura de la ranura PoH antes de que el líder actual termine emitiendo todo el bloque.

El próximo líder tiene que tomar la decisión de adjuntar su propio bloque al último bloque completado, o esperar para finalizar el bloque pendiente. Es posible que el próximo líder produzca un bloque que proponga que el actual líder fracase, aunque el resto de la red observa que el bloque tiene éxito.

El líder actual tiene incentivos para empezar su franja horaria lo antes posible para obtener recompensas económicas. Esos incentivos deben equilibrarse con la necesidad del líder de fijar su bloque a un bloque que tenga el mayor compromiso del resto de la red.

## Tiempo de espera del líder

Mientras que un líder está recibiendo activamente entradas para la rama anterior, el líder puede retrasar la transmisión del inicio de su bloque en tiempo real. El retraso es configurable localmente por cada líder, y puede basarse dinámicamente en el comportamiento del líder anterior. Si el bloque anterior del líder es confirmado por el TVU del líder antes del tiempo de espera, el PoH se reinicia al inicio de la ranura y este líder produce su bloque inmediatamente.

Las desventajas:

- El líder retrasa su propio espacio, permitiendo potencialmente al próximo líder más tiempo

  para ponerse al día.

Los aspectos positivos en comparación con los guardias:

- Todo el espacio de un bloque se usa para entradas.
- No se ha solucionado el tiempo de espera.
- El tiempo de espera es local para el líder, y por lo tanto puede ser inteligente. La heurística del líder puede tener en cuenta el rendimiento de la turbina.
- Este diseño no requiere una bifurcación de contabilidad para actualizar.
- El líder anterior puede transmitir redundamente la última entrada en el bloque al siguiente líder, y el próximo líder puede decidir especulativamente confiar en él para generar su bloque sin verificar el bloque anterior.
- El líder puede generar especulativamente el último tick de la última entrada recibida.
- El líder puede procesar de forma especulativa las transacciones y adivinar cuáles no van a ser codificadas por el líder anterior. Este es también un vector de ataque de la censura. El líder actual puede retener las transacciones que recibe de los clientes para que pueda codificarlas en su propia ranura. Una vez procesadas, las entradas pueden ser reproducidas en PoH rápidamente.

## Opciones de diseño alternativo

### Guardián al final de la ranura

Un líder no produce entradas en su bloque después del pulso _penultimate tick_, que es el último tick antes del primer tick de la siguiente ranura. La red vota en el _last tick_, así que la diferencia de tiempo entre el pulso _penultimate tick_ y el _last tick_ es el retraso forzado para toda la red, así como el próximo líder antes de que se pueda generar una nueva ranura. La red puede producir el _last tick_ del _penultimate tick_.

Si el próximo líder recibe el _penultimate tick_ antes de que produzca su propio _first tick_, reiniciará su PoH y producirá el _first tick_ del anterior líder _penultimate tick_. El resto de la red también restablecerá su PoH para producir el _último tick_ como el id para votar.

Las desventajas:

- Cada votación y, por lo tanto, cada confirmación se retrasa por un tiempo de espera fijo. 1 tick, o alrededor de 100m.
- El tiempo promedio de confirmación de caso para una transacción sería de al menos 50 ms peor.
- Es parte de la definición del libro mayor, por lo que cambiar este comportamiento requeriría una bifurcación dura.
- No todo el espacio disponible se utiliza para las entradas.

Las ventajas en comparación con el tiempo de espera del líder:

- El próximo líder ha recibido todas las entradas anteriores, por lo que puede comenzar a procesar transacciones sin registrarlas en PoH.
- El líder anterior puede transmitir redundamente la última entrada que contiene el pulso _penultimate_ al siguiente líder. El próximo líder puede generar especulativamente el _último tick_ tan pronto como reciba el _penúltimo tick_, incluso antes de verificarlo.
