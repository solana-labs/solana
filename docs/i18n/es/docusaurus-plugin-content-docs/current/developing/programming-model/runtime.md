---
title: "Tiempo de ejecución"
---

## Capacidad de los programas

El tiempo de ejecución sólo permite al programa propietario debitar la cuenta o modificar sus datos. A continuación, el programa define reglas adicionales para que el cliente pueda modificar las cuentas que posee. En el caso del programa Sistema, permite a los usuarios transferir lamports mediante el reconocimiento de las firmas de las transacciones. Si ve que el cliente firmó la transacción usando la _clave privada_ del keypair, sabe que el cliente autorizó la transferencia del token.

En otras palabras, todo el conjunto de cuentas que posee un determinado programa puede considerarse como un almacén de clave-valor en el que una clave es la dirección de la cuenta y el valor son datos binarios arbitrarios específicos del programa. El autor de un programa puede decidir cómo gestionar todo el estado del programa como posiblemente muchas cuentas.

Después de que el tiempo de ejecución ejecuta cada una de las instrucciones de la transacción, utiliza los metadatos de la cuenta para verificar que no se ha violado la política de acceso. Si un programa viola la política, el tiempo de ejecución descarta todos los cambios de cuenta realizados por todas las instrucciones de la transacción y marca la transacción como fallida.

### Política

Después de que un programa haya procesado una instrucción, el tiempo de ejecución verifica que el programa sólo ha realizado las operaciones que se le han permitido y que los resultados se ajustan a la política de tiempo de ejecución.

La política es la siguiente:

- Sólo el dueño de la cuenta puede cambiar de propietario.
  - Y sólo si la cuenta es escribible.
  - Y sólo si la cuenta no es ejecutable
  - Y sólo si los datos son nulos o están vacíos.
- Una cuenta no asignada al programa no puede disminuir su saldo.
- El balance de las cuentas de sólo lectura y las ejecutables no puede cambiar.
- Sólo el programa del sistema puede cambiar el tamaño de los datos y sólo si el programa del sistema es el propietario de la cuenta.
- Sólo el propietario puede modificar los datos de la cuenta.
  - Y si la cuenta es escribible.
  - Y si la cuenta no es ejecutable.
- Executable es unidireccional (false->true) y sólo el propietario de la cuenta puede establecerlo.
- No hay nadie que modifique el rent_epoch asociado con esta cuenta.

## Calcular presupuesto

Para evitar que un programa abuse de los recursos de cálculo, cada instrucción de una transacción recibe un presupuesto de cálculo. El presupuesto se compone de unidades de cálculo que se consumen a medida que el programa realiza diversas operaciones y de límites que el programa no puede superar. Cuando el programa consume todo su presupuesto o supera un límite entonces el tiempo de ejecución detiene el programa y devuelve un error.

Las siguientes operaciones tienen un costo de cálculo:

- Ejecutando instrucciones BPF
- Llamadas al sistema
  - ingresando
  - creando direcciones del programa
  - invocaciones entre programas
  - ...

Para invocaciones entre programas, los programas invocados heredan el presupuesto de su padre. Si un programa invocado consume el presupuesto o excede un límite total de la cadena de invocación y el padre se detiene.

El actual [presupuesto budget](https://github.com/solana-labs/solana/blob/d3a3a7548c857f26ec2cb10e270da72d373020ec/sdk/src/process_instruction.rs#L65) se puede encontrar en el SDK de Solana.

Por ejemplo, si el presupuesto actual es:

```rust
max_units: 200,000,
log_units: 100,
log_u64_units: 100,
create_program address units: 1500,
invoke_units: 1000,
max_invoke_depth: 4,
max_call_depth: 64,
stack_frame_size: 4096,
log_pubkey_units: 100,
```

Entonces el programa

- Puede ejecutar 200,000 instrucciones BPF si no hace nada más
- Puede registrar 2.000 mensajes de registro
- No se puede exceder 4k del uso de la pila
- No se puede exceder una profundidad de llamada BPF de 64
- No se pueden exceder los 4 niveles de invocaciones entre programas.

Dado que el presupuesto de computación se consume de forma incremental a medida que el programa se ejecuta, el consumo total del presupuesto será una combinación de los distintos costes de las operaciones que realiza.

En tiempo de ejecución, un programa puede registrar la cantidad de presupuesto de cómputo que queda. Consulte [debugging](developing/deployed-programs/debugging.md#monitoring-compute-budget-consumption). para obtener más información.

Los valores del presupuesto están condicionados a la habilitación de la característica, eche un vistazo a la función [nueva](https://github.com/solana-labs/solana/blob/d3a3a7548c857f26ec2cb10e270da72d373020ec/sdk/src/process_instruction.rs#L97) del presupuesto para saber cómo se construye el presupuesto. Para determinar los valores del presupuesto actual es necesario conocer cómo funcionan las [características](runtime.md#features) y qué características están activadas en el clúster que se está utilizando.

## Características nuevas

A medida que Solana evoluciona, se pueden introducir nuevas características o parches que cambien el comportamiento del clúster y cómo se ejecutan los programas. Los cambios de comportamiento deben ser coordinados entre los distintos nodos del cluster, si los nodos no se coordinan entonces estos cambios pueden dar lugar a una ruptura del consenso. Solana soporta un mecanismo llamado características de tiempo de ejecución para facilitar la adopción fluida de cambios.

Las características de tiempo de ejecución son eventos coordinados por épocas en los que se producirán uno o más cambios de comportamiento en el clúster. Los nuevos cambios en Solana que cambiarán el comportamiento están envueltos con puertas de características y deshabilitados por defecto. A continuación, se utilizan las herramientas de Solana para activar una característica, que la marca como pendiente; una vez marcada como pendiente, la característica se activará en la siguiente época.

Para determinar qué características están activadas, utilice [las herramientas de línea de comandos de Solana](cli/install-solana-cli-tools.md):

```bash
estado de la característica solana
```

Si encuentra problemas, primero asegúrese de que la versión de Solana tools que está usando coincide con la versión devuelta por `solana cluster-version`. Si no coinciden con [instale la suite de herramientas correcta](cli/install-solana-cli-tools.md).
