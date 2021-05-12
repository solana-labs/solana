---
title: Corrección de la marca de tiempo del banco
---

Cada banco tiene una marca de tiempo que se almacena en el reloj sysvar y se utiliza para evaluar bloqueos de cuenta de acciones basados en el tiempo. Sin embargo, como el génesis, este valor ha estado basado en una ranura teórica-por-segundo en lugar de la realidad, así que es bastante inpreciso. Esto plantea un problema para los bloqueos, ya que las cuentas no se registrarán como sin bloqueo en (o en cualquier momento cercano) la fecha en la que el bloqueo se establece en expirar.

Los tiempos de bloque ya se están calculando para almacenar caché en Blockstore y almacenamiento a largo plazo usando un [oráculo de fecha del validador](validator-timestamp-oracle.md); estos datos proporcionan una oportunidad para alinear la marca de tiempo del banco más estrechamente con tiempo del mundo real.

El esquema general de la aplicación propuesta es el siguiente:

- Corregir cada marca de tiempo del Banco usando la marca de tiempo proporcionada por el validador.
- Actualizar el cálculo de la marca de tiempo proporcionada por el validador para usar una mediana ponderada por la apuesta, en lugar de una media ponderada por la apuesta.
- Se ha vinculado la corrección de la marca de tiempo para que no pueda desviarse demasiado lejos de la estimación teórica esperada

## Corrección de marca de tiempo

En cada nuevo Banco, el tiempo de ejecución calcula una estimación realista de la marca de tiempo usando datos de timestamp-oracle del validador. La marca de tiempo del Banco se corrige a este valor si es mayor o igual a la marca de tiempo anterior del Banco. Es decir, el tiempo no debería volver atrás, para que las cuentas bloqueadas puedan ser liberadas por la corrección, pero una vez liberadas, las cuentas nunca pueden ser rebloqueadas por un tiempo de corrección.

### Cálculo de la marca de tiempo mediana ponderada por participación

Para calcular la fecha estimada de un banco en particular, el tiempo de ejecución primero necesita obtener las marcas de tiempo de votación más recientes del validador activo. El método `Bank::vote_accounts()` proporciona el estado de las cuentas de voto, y estos pueden ser filtrados a todas las cuentas cuyo timestamp más reciente fue proporcionado dentro del último epoch.

Desde cada marca de tiempo de voto, una estimación para el Banco actual se calcula usando el objetivo ns_per_slot del epoch para cualquier delta entre la ranura del Banco y la ranura de la marca de tiempo. Cada estimación de marca de tiempo está asociada con la participación delegada a esa cuenta de voto, y todas las marcas de tiempo se recopilan para crear una distribución de la marca de tiempo ponderada por participación.

A partir de este conjunto, la marca de tiempo mediana ponderada por participación, es decir, la marca de tiempo en cuyo 50% de la participación estima una marca de tiempo mayor o igual y el 50% de la la participación estima una marca de tiempo menor o igual: se selecciona como el potencial marca de tiempo corregida.

Esta marca de tiempo mediana ponderada por la apuesta, es preferible sobre la media ponderada por la apuesta, porque la multiplicación de la participación por marca de tiempo propuesta en el cálculo del promedio permite que un nodo con una apuesta muy pequeña siga teniendo un efecto grande en la marca de tiempo resultante proponiendo una marca de tiempo muy grande o muy pequeña. Por ejemplo, utilizando el método anterior `calculate_stake_weighted_timestamp()`, un nodo con 0. ¡0003% de la apuesta proponiendo una marca de tiempo de `i64::MAX` puede desplazar la marca de tiempo hacia adelante 97k años!

### Marcas de tiempo delimitadas

Además de prevenir el movimiento del tiempo hacia atrás, podemos prevenir la actividad maliciosa limitando la marca de tiempo corregida a un nivel aceptable de desviación del tiempo esperado teóricamente.

Esta propuesta sugiere que se permita que cada marca de tiempo se desvíe hasta un 25% de el tiempo esperado desde el inicio de la época.

Para calcular la desviación de la marca de tiempo, cada banco necesita registrar la `epoch_start_timestamp` en el sysvar de Reloj. Este valor está establecido en `Clock::unix_timestamp` en la primera ranura de cada epoch.

Luego, el tiempo de ejecución compara el tiempo esperado transcurrido desde el inicio del epoch con el tiempo de ejecución propuesto basado en la marca de tiempo corregida. Si el tiempo corregido está dentro de +/- 25% del esperado, la marca de tiempo corregida es aceptada. De lo contrario, se limita a la desviación aceptable.
