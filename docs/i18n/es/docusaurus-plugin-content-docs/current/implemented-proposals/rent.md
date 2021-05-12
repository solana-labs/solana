---
title: Rentar
---

Las cuentas en Solana pueden tener un estado controlado por el dueño \(`Account::data`\) que está separado del saldo de la cuenta \(`Account::lamports`\). Dado que los validadores de la red necesitan mantener una copia de trabajo de este estado en la memoria, la red cobra una comisión basada en tiempo y espacio para este consumo de recursos, también conocido como Rent.

## Régimen de renta en dos niveles

Las cuentas que mantienen un saldo mínimo equivalente a 2 años de pagos por alquiler son exentas. Los _2 years_ se extrae del hecho de que el coste del hardware baja un 50% de precio cada 2 años y de la convergencia resultante por ser una serie geométrica. Las cuentas cuyo saldo cae por debajo de este umbral se cobran a una tasa especificada en en el genesis, en los lamports por byte-año. La red cobra el alquiler por epoch, en crédito para el próximo epoch, y `Account::rent_epoch` mantiene un seguimiento de la próxima vez que el alquiler debe ser cobrado de la cuenta.

Actualmente, el costo del alquiler se fija en el genesis. Sin embargo, es anticipado a ser dinámico, reflejando el costo de almacenamiento de hardware subyacente en ese momento. De modo que generalmente se espera que el precio disminuya a medida que la tecnología avance.

## Plazos de cobro del alquiler

Hay dos momentos de cobro de la renta de las cuentas: \(1\) cuando se refiere a una transacción, \(2\) periódicamente una vez por época. \(1\) incluye la transacción para crear la nueva cuenta en sí, y ocurre durante el procesamiento normal de la transacción por el banco como parte de la fase de carga. \(2\) existe para asegurar el cobro de las rentas de las cuentas antiguas, a las que no se hace referencia en las épocas recientes. \(2\) requiere el escaneo completo de las cuentas y se reparte a lo largo de una época en función del prefijo de la dirección de la cuenta para evitar picos de carga debido a esta recogida de rentas.

Por el contrario, la recaudación de alquileres no se aplica a cuentas que son directamente manipuladas por cualquiera de los procesos de contabilidad a nivel de protocolos, incluyendo:

- La distribución de la propia colección de alquileres (de lo contrario, puede causar un manejo recursivo de la colección de alquileres)
- La distribución de las recompensas de stake al comienzo de cada época (Para reducir al máximo el pico de procesamiento al comienzo de la nueva época)
- La distribución de la cuota de transacción al final de cada ranura

Incluso si esos procesos están fuera del alcance de la recaudación de rentas, todas las cuentas manipuladas serán eventualmente manejadas por el mecanismo\(2\).

## Procesamiento actual de la recolección del alquiler

El alquiler se debe a una época, y las cuentas tienen `Account::rent_epoch` de`current_epoch` or `current_epoch + 1` dependiendo del régimen de alquiler.

Si la cuenta está en el régimen exento, `Account::rent_epoch` simplemente se actualiza a `current_epoch`.

Si la cuenta no está exenta, la diferencia entre la siguiente época y `Account::rent_epoch` se utiliza para calcular la cantidad de alquiler que debe esta cuenta (a través de `Rent::due()`\). Cualquier lamport fraccional del cálculo es truncada. La renta adeudada se deduce de `Account::lamports` y `Account::rent_epoch` se actualiza a `current_epoch + 1` (= próxima época). Si el importe del alquiler es inferior a un lamport, no se realizarán cambios en la cuenta.

Las cuentas cuyo saldo es insuficiente para satisfacer el alquiler que se deba simplemente no se cargan.

Un porcentaje del alquiler recolectado es destruido. El resto se distribuye a cuentas de validadores por el peso del stake, a las comisiones de transacción, al final de cada ranura.

Por último, la recaudación de alquileres se realiza de acuerdo con las actualizaciones de la cuenta a nivel protocolar como la distribución de alquileres a validadores, lo que significa que no hay una transacción correspondiente para las deducciones de alquiler. Así, la recaudación de alquileres es bastante invisible, sólo implícitamente observable por una transacción reciente o temporización predeterminada dado su prefijo de dirección de cuenta.

## Consideraciones de diseño

### Justificación del diseño actual

Con el diseño anterior, NO es posible tener cuentas que permanezcan, que nunca se toquen y que nunca tengan que pagar la renta. Accounts always pay rent exactly once for each epoch, except rent-exempt, sysvar and executable accounts.

This is an intended design choice. Otherwise, it would be possible to trigger unauthorized rent collection with `Noop` instruction by anyone who may unfairly profit from the rent (a leader at the moment) or save the rent given anticipated fluctuating rent cost.

As another side-effect of this choice, also note that this periodic rent collection effectively forces validators not to store stale accounts into a cold storage optimistically and save the storage cost, which is unfavorable for account owners and may cause transactions on them to stall longer than others. On the flip side, this prevents malicious users from creating significant numbers of garbage accounts, burdening validators.

As the overall consequence of this design, all accounts are stored equally as a validator's working set with the same performance characteristics, reflecting the uniform rent pricing structure.

### Colección Ad-hoc

Se consideró la posibilidad de cobrar el alquiler en función de las necesidades (es decir, cada vez que se cargan/acceden las cuentas). Los problemas de este enfoque son:

- las cuentas cargadas como "sólo crédito" para una transacción podrían esperar muy razonablemente que tuvieran alquileres adeudados,

  pero no se puede escribir durante ninguna transacción de este tipo

- un mecanismo de "golpear los arbustos" (es decir, ir a buscar las cuentas que necesitan pagar la renta)es conveniente,

  no sea que las cuentas que se cargan con poca frecuencia consigan un viaje gratis

### Instrucciones del sistema para la recogida del alquiler

Se consideró la posibilidad de recaudar la renta a través de una instrucción del sistema, ya que, naturalmente, habría distribuido la renta entre los nodos activos y ponderados por la participación y podría haberse hecho de forma incremental. Sin embargo:

- habría afectado negativamente al rendimiento de la red
- requeriría una carcasa especial por parte del tiempo de ejecución, ya que las cuentas con propietarios no pertenecientes a SystemProgram pueden ser cargadas por esta instrucción
- alguien tendría que emitir las transacciones
