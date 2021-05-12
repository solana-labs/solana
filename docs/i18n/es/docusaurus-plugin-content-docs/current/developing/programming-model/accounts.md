---
title: "Cuentas"
---

## Almacenando Estado entre Transacciones

Si el programa necesita almacenar el estado entre las transacciones, lo hace usando _cuentas_. Las cuentas son similares a archivos en sistemas operativos como Linux. Al igual que un archivo, una cuenta puede contener datos arbitrarios y esos datos persisten más allá de la vida útil de un programa. También como un archivo, una cuenta incluye metadatos que le dice al tiempo de ejecución quién puede acceder a los datos y cómo.

A diferencia de un archivo, la cuenta incluye metadatos para la vida útil del archivo. Que el tiempo de vida se expresa en "tokens", que es un número de tokens nativo fraccional, llamado _lamports_. Las cuentas se mantienen en memoria del validador y pagan ["renta"](#rent) para permanecer allí. Cada validador analiza periódicamente todas las cuentas y cobra alquiler. Cualquier cuenta que cae a cero lamports es purgada. Las cuentas también pueden marcarse como [rent-exempt](#rent-exemption) si contienen un número de lamports.

Del mismo modo que un usuario de Linux usa una ruta para buscar un archivo, un cliente Solana utiliza una _dirección_ para buscar una cuenta. La dirección es una clave pública de 256 bits.

## Firmadores

Las transacciones pueden incluir [firmas digitales](terminology.md#signature) correspondientes a las claves públicas de las cuentas referenciadas por la transacción. Cuando la firma digital correspondiente está presente, significa que el titular de la clave privada de la cuenta firmó y, por tanto, "autorizó" la transacción y la cuenta se denomina entonces como _firmante_. Si una cuenta es un firmante o no se comunica al programa como parte de los metadatos de la cuenta. Los programas pueden usar esa información para tomar decisiones de autoridad.

## Sólo lectura

Las transacciones pueden [indicar](transactions.md#message-header-format) que algunas de las cuentas que hace referencia son tratadas como _cuentas de solo lectura_ para permitir procesamiento de cuentas paralelas entre transacciones. El tiempo de ejecución permite que las cuentas de solo lectura sean leídas simultáneamente por varios programas. Si un programa intenta modificar una cuenta de solo lectura, la transacción es rechazada por el tiempo de ejecución.

## Ejecutable

Si una cuenta está marcada como "ejecutable" en sus metadatas, entonces se considera un programa que puede ejecutarse incluyendo la clave pública de la cuenta una instrucción [id de programa](transactions.md#program-id). Los clientes son marcados como ejecutables durante un proceso de despliegue exitoso del programa por el cargador que posee la cuenta. Por ejemplo, durante el despliegue del programa BPF, una vez que el cargador ha determinado que el bytecode BPF en los datos de la cuenta es válido, el cargador marca permanentemente la cuenta del programa como ejecutable. Una vez ejecutable, el tiempo de ejecución impone que los datos de la cuenta (el programa) son inmutables.

## Creando

Para crear una cuenta, un cliente genera un keypair \_\_ y registra su clave pública usando la instrucción `SystemProgram::CreateAccount` con un tamaño de almacenamiento fijo en bytes. El tamaño máximo actual de los datos de una cuenta es de 10 megabytes.

Una dirección de cuenta puede ser cualquier valor arbitrario de 256 bits, y hay mecanismos para usuarios avanzados para crear direcciones derivadas (`SystemProgram::CreateAccountWithSeed`, [`Pubkey::CreateProgramAddress`](calling-between-programs.md#program-derived-addresses)).

Las cuentas que nunca han sido creadas a través del programa del sistema también pueden pasarse a los programas. Cuando una instrucción hace referencia a una cuenta que no ha sido creada previamente, se le pasará al programa una cuenta que es propiedad del programa del sistema, tiene cero lamports y cero datos. Pero, la cuenta reflejará si es un firmante de la transacción o no y, por lo tanto, puede ser utilizada como una autoridad. Las autoridades en este contexto transmiten al programa que el titular de la clave privada asociada a la clave pública de la cuenta firmó la transacción. La clave pública de la cuenta puede ser conocida por el programa o registrada en otra cuenta y significa algún tipo de propiedad o autoridad sobre un activo o el funcionamiento del programa controla o realiza.

## Propiedad y asignación a programas

Una cuenta creada es inicializada para ser _propiedad_ de un programa integrado llamado el programa de sistema y se llama una _cuenta de sistema_ apropiadamente. Una cuenta incluye metadatos "propietarios". El propietario es un id del programa. El tiempo de ejecución otorga al programa permisos de escritura a la cuenta si su id coincide con el propietario. Para el caso del programa del sistema, el tiempo de ejecución permite a los clientes transferir lamports y es importante _asignar_ la propiedad de la cuenta, significa cambiar el propietario a otro id de programa. Si una cuenta no es propiedad de un programa, el programa solo puede leer sus datos y acreditar la cuenta.

## Renta

Mantener las cuentas vivas en Solana incurre en un costo de almacenamiento llamado _renta_ porque el clúster debe mantener activamente los datos para procesar cualquier transacción futura en ella. Esto es diferente de Bitcoin y Ethereum, donde las cuentas de almacenamiento no incurren en ningún costo.

La renta es cargada del saldo de una cuenta por el tiempo de ejecución en el primer acceso (incluyendo la creación inicial de la cuenta) en la época actual por las transacciones o una vez por una época si no hay transacciones. La comisión es actualmente una tasa fija, medida en bytes-times-epochs. La comisión puede cambiar en el futuro.

Para facilitar el cálculo de la renta, ésta se cobra siempre para una sola época completa. El alquiler no se prorratea, lo que significa que no hay tasas ni reembolsos por épocas parciales. Esto significa que, al crear la cuenta, la primera renta cobrada no es para la época parcial actual, sino que se cobra por adelantado para la siguiente época completa. Los cobros de renta posteriores son para otras épocas futuras. Por otro lado, si el saldo de una cuenta ya cobrada cae por debajo de otra cuota de alquiler a mitad de época, la cuenta seguirá existiendo durante la época actual y se purgará inmediatamente al comienzo de la siguiente.

Las cuentas pueden estar exentas del pago del alquiler si mantienen un saldo mínimo. Esta exención de alquiler se describe a continuación.

### Calculación del alquiler

Nota: La tarifa de alquiler puede cambiar en el futuro.

A partir de la escritura, la tarifa fija de alquiler es 19.055441478439427 lamports por byte-epoch en la red de pruebas y clusters mainnet-beta. Una [época ](terminology.md#epoch) tiene como objetivo ser de 2 días (Para devnet, la tasa de alquiler 0,3608183131797095 lamports por byte-epoc con su época de 54m36s de duración).

Este valor se calcula para el objetivo 0.01 SOL por mebibyte-day (exactamente coincidiendo a 3.56 SOL por mebibyte-año):

```text
Tarifa de alquiler: 19.055441478439427 = 10_000_000 (0.01 SOL) * 365(aprox. día en un año) / (1024 * 1024)(1 MiB) / (365.25/2)(epocas en 1 año)
```

Y el cálculo del alquiler se hace con la precisión `f64` y el resultado final se trunca a `u64` en lamports.

El cálculo del alquiler incluye metadatos de la cuenta (dirección, propietario, lámports, etc) en el tamaño de una cuenta. Por lo tanto, la cuenta más pequeña puede ser para el alquiler cálculos es de 128 bytes.

Por ejemplo, se crea una cuenta con la transferencia inicial de 10.000 lamports y sin datos adicionales. El alquiler se debitará inmediatamente al crear, resultando en un saldo de 7,561 lamports:

```text
Alquiler: 2.439 = 19,055441478439427 (tasa de alquiler) * 128 bytes (tamaño mínimo de la cuenta) * 1 (época) Saldo de la cuenta: 7.561 = 10.000 (lamports transferidas) - 2.439 (tasa de alquiler de esta cuenta para una época)
```

El saldo de la cuenta se reducirá a 5,122 lamports en la siguiente época incluso si no hay actividad:

```text
Saldo de la cuenta: 5.122 = 7.561 (saldo actual) - 2.439 (cuota de alquiler de esta cuenta por una época)
```

En consecuencia, una cuenta de tamaño mínimo será inmediatamente eliminada después de la creación si los lamports transferidos son menores o iguales a 2,439.

### Exención de alquiler

Alternativamente, se puede hacer una cuenta completamente exenta de la recolección de alquileres depositando al menos 2 años de alquiler. Esto se comprueba cada vez que se reduce el saldo de una cuenta y el alquiler se debita inmediatamente una vez que el saldo se encuentre por debajo de la cantidad mínima.

Las cuentas ejecutables del programa son requeridas por el tiempo de ejecución para ser rent-exempt a evitar ser purgadas.

Nota: Utilice el endpoint [`getMinimumBalanceForRentExemption` RPC ](developing/clients/jsonrpc-api.md#getminimumbalanceforrentexemption) para calcular el balance mínimo para un tamaño de cuenta en particular. El siguiente cálculo es solo ilustrativo.

Por ejemplo, un programa ejecutable con el tamaño de 15,000 bytes requiere un saldo de 105,290,880 lamports (=~ 0.105 SOL) para ser rent-exempt:

```text
105,290,880 = 19.055441478439427 (tarifa de comisión) * (128 + 15_000)(tamaño de la cuenta incluyendo metadatos) * ((365.25/2) * 2)(épocas en 2 años)
```
