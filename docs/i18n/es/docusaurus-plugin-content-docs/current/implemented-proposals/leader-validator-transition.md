---
title: Transición de líder a validador
---

Un validador normalmente pasa su tiempo validando bloques. Sin embargo, si un staker delega su stake a un validador, de vez en cuando será seleccionado como un líder de _slot_. Como líder de una ranura, el validador es responsable de producir bloques durante una _ranura_ asignada. Una ranura tiene una duración de algún número de _ticks_ preconfigurados. La duración de esos ticks se calcula con una _Grabadora de PoH_ descrita más adelante en este documento.

## BankFork

BankFork rastrea los cambios en el estado del banco a través de una ranura específica. Una vez que el tick final ha sido registrado, el estado está congelado. Cualquier intento de escribir será rechazado.

## Validador

Un validador opera en muchos forks concurrentes diferentes del estado del banco hasta que genera un hash PoH con una altura dentro de su ranura líder.

## Líder de la ranura

Un líder de la ranura construye bloques en la parte superior de un fork, el que votó por última vez.

## Grabador PoH

Los líderes y validadores de las ranuras utilizan un Grabador PoH para estimar tanto la altura de la ranura como para grabar transacciones.

### Grabadora PoH al validar

La Grabadora PoH actúa como un simple VDF al validar. Indica al validador cuándo debe cambiar al papel de líder de ranura. Cada vez que el validador vota en un fork, debe utilizar el último [blockhash](../terminology.md#blockhash) del fork para resembrar el VDF. La resiembra resuelve dos problemas. En primer lugar, sincroniza su VDF con el del líder, permitiéndole determinar con mayor precisión cuándo comienza su ranura de líder. En segundo lugar, si el líder anterior cae, todo el tiempo del reloj se tiene en cuenta en el flujo de PoH del siguiente líder. Por ejemplo, si falta un bloque cuando se inicia el líder, el bloque que produce debe tener una duración PoH de dos bloques. La duración más larga asegura que el siguiente líder no intente cortar todas las transacciones de la ranura del líder anterior.

### Grabadora PoH al liderar

Un líder de la ranura utiliza la Grabadora de PoH para registrar transacciones, bloqueando sus posiciones a tiempo. El hash de PoH debe derivarse del último bloque de un líder anterior. Si no lo es, su bloque fallará la verificación PoH y será rechazado por el cluster.

El grabador del PoH también sirve para informar al líder de la ranura cuando ésta ha terminado. El líder debe tener cuidado de no modificar su banco si el registro de la transacción genera una altura PoH fuera de su ranura designada. El líder, por lo tanto, no debe comprometer los cambios de cuenta hasta después de que genere el hash de la entrada. Cuando la altura del PoH cae fuera de su ranura cualquier transacción en su pipeline puede caer o ser enviada al siguiente líder. Se prefiere reenviar, ya que minimizaría la congestión de la red, permitiendo que el clúster anuncie una mayor capacidad de TPS.

## Bucle de validación

La Grabadora PoH gestiona la transición entre modos. Una vez que se repite un ledger, el validador puede correr hasta que la grabadora indique que debe ser el líder de la ranura. Como líder de una ranura, el nodo puede entonces ejecutar y registrar transacciones.

El bucle se sincroniza con PoH y hace un inicio y una parada sincrónica de la funcionalidad de líder de la ranura. Después de parar, el TVU del validador debe encontrarse en el mismo estado que si un líder diferente lo había enviado el mismo bloque. Lo siguiente es el pseudocódigo para el bucle:

1. Consulta al Programador de Líderes para la siguiente ranura asignada.
2. Ejecute el TVU sobre todos los forks. 1. TVU enviará votos a lo que cree que es el "mejor" fork. 2. Después de cada votación, reinicie el grabador PoH para que funcione hasta la siguiente ranura

   asignada.

3. Cuando sea el momento de ser un líder de ranura, inicie el TPU. Apunta al ultimo fork

   Que votó la TVU.

4. Produce entradas hasta el final de la ranura. 1. Durante la duración del slot, el VTU no debe votar sobre otros forks. 2. Una vez finalizada la ranura, el TPU congela su BankFork. Después de congelar,

   el TVU puede reanudar la votación.

5. Ir a 1.
