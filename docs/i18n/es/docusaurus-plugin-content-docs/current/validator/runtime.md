---
title: El tiempo de ejecución
---

## El tiempo de ejecución

El tiempo de ejecución es un procesador de transacciones simultáneo. Las transacciones especifican sus dependencias de datos y la asignación de memoria dinámica es explícita. Separando el código del programa del estado en el que opera, el tiempo de ejecución es capaz de coreografiar el acceso simultáneo. Las transacciones que sólo acceden a las cuentas de sólo lectura se ejecutan en paralelo, mientras que las transacciones que acceden a las cuentas de escritura se ejecutan en serie. El tiempo de ejecución interactúa con el programa a través de un punto de entrada con una interfaz bien definida. Los datos almacenados en una cuenta son un tipo opaco, un array de bytes. El programa tiene control total sobre sus contenidos.

La estructura de la transacción especifica una lista de claves públicas y firmas para esas claves y una lista secuencial de instrucciones que operarán sobre los estados asociados a las claves de las cuentas. Para que la transacción se confirme, todas las instrucciones deben ejecutarse con éxito; si alguna falla, toda la transacción no se confirma.

#### Estructura de Cuenta

Las cuentas mantienen un balance de lamps y memoria específica del programa.

## Motor de transacciones

El motor asigna las claves públicas a cuentas y las lleva al punto de entrada del programa.

### Ejecución

Las transacciones se procesan por lotes y se procesan en un pipeline. El TPU y el TVU siguen un camino ligeramente diferente. El tiempo de ejecución TPU asegura que el registro PoH ocurra antes de que la memoria sea confirmada.

El tiempo de ejecución TVU asegura que la verificación PoH se produzca antes de que el tiempo de ejecución procese cualquier transacción.

![Tiempo de ejecución del pipeline](/img/runtime.svg)

En la etapa _ejecutar_, las cuentas cargadas no tienen dependencias de datos, por lo que todos los programas pueden ejecutarse en paralelo.

El tiempo de ejecución impone las siguientes reglas:

1. Solo el programa _propietario_ puede modificar el contenido de una cuenta. Esto significa que al asignar el vector de datos está garantizado a ser cero.
2. Los saldos totales en todas las cuentas son iguales antes y después de la ejecución de una transacción.
3. Después de ejecutar la transacción, los saldos de las cuentas de sólo lectura deben ser iguales a los saldos antes de la transacción.
4. Todas las instrucciones de la transacción ejecutada atómicamente. Si uno falla, todas las modificaciones de la cuenta serán descartadas.

La ejecución del programa implica mapear la clave pública del programa a un punto de entrada que toma un puntero a la transacción, y un array de cuentas cargadas.

### Interfaz de programa del sistema

La interfaz es mejor descrita por la `Instruction::data` que el usuario codifica.

- `CreateAccount` - Permite al usuario crear una cuenta con una matriz de datos asignada y asignarla a un Programa.
- `CreateAccountWithSeed` - Igual que `CreateAccount`, pero la dirección de la nueva cuenta deriva de
  - el pubkey de la cuenta de fondos,
  - una cadena mnemónica (semilla), y
  - la pubkey del programa
- `Asignar` - Permite al usuario asignar una cuenta existente a un programa.
- `Transferencia` - Transfiere lamports entre cuentas.

### Programa de Seguridad de Estado

Para que la blockchain funcione correctamente, el código del programa debe ser resistente a las entradas del usuario. Por eso en este diseño el código específico del programa es el único código que puede cambiar el estado de la matriz de bytes de datos en los Cuentas que se le asignan. También es la razón por la que `Asignar` o `CrearCuenta` deben poner a cero los datos. De lo contrario, no habría forma de que el programa distinguiera los datos de la cuenta recientemente asignados de una transición de estado generada de forma nativa sin algunos metadatos adicionales del tiempo de ejecución para indicar que esta memoria es asignada en lugar de generada de forma nativa.

Para pasar mensajes entre programas, el programa receptor debe aceptar el mensaje y copiar el estado de nuevo. Pero en la práctica una copia no es necesaria y es inaceptable. El programa receptor puede leer el estado de otros clientes sin copiarlo, y durante la lectura tiene una garantía del estado del programa del remitente.

### Notas

- No hay asignación dinámica de memoria. El cliente necesita usar `CreateAccount` instrucciones para crear memoria antes de pasarla a otro programa. Esta instrucción se puede componer en una sola operación con la llamada al propio programa.
- `CrearCuenta` y `Asignar` garantizan que cuando se asigna la cuenta al programa, los datos de la Cuenta se inicializan a cero.
- Las transacciones que asignan una cuenta a un programa o asignan espacio deben estar firmadas por la clave privada de la dirección de la cuenta a menos que la cuenta esté siendo creada por `CreateAccountWithSeed`, en cuyo caso no hay una clave privada correspondiente para la dirección/pubkey de la cuenta.
- Una vez asignado al programa no se puede reasignar una cuenta.
- El tiempo de ejecución garantiza que el código de un programa es el único que puede modificar los datos de la Cuenta a la que está asignada.
- El tiempo de ejecución garantiza que el programa sólo puede gastar lamports que están en las cuentas que se le asignan.
- El tiempo de ejecución garantiza que los saldos de las cuentas estén equilibrados antes y después de la transacción.
- El tiempo de ejecución garantiza que todas las instrucciones se ejecuten con éxito cuando se confirma una transacción.

## Trabajo futuro

- [Continuaciones y señales para transacciones de larga duración](https://github.com/solana-labs/solana/issues/1485)
