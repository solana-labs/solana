---
title: Llamada entre programas
---

## Invocaciones interprogramas

El tiempo de ejecución Solana permite a los programas llamarse unos a otros a través de un mecanismo llamado invocación entre programas. La llamada entre programas se logra mediante un programa que invoca una instrucción del otro. El programa de invocación se detiene hasta que el programa invocado termine de procesar la instrucción.

Por ejemplo, un cliente podría crear una transacción que modifique dos cuentas, cada una de ellas propiedad de programas separados en cadena:

```rust,ignore
let message = Message::new(vec![
    token_instruction::pay(&alice_pubkey),
    acme_instruction::launch_missiles(&bob_pubkey),
]);
client.send_and_confirm_message(&[&alice_keypair, &bob_keypair], &message);
```

Un cliente puede, en cambio, permitir que el programa `acme` invoque convenientemente las instrucciones del `token` en nombre del cliente:

```rust,ignore
let message = Message::new(vec![
    acme_instruction::pay_and_launch_missiles(&alice_pubkey, &bob_pubkey),
]);
client.send_and_confirm_message(&[&alice_keypair, &bob_keypair], &message);
```

Dados dos programas en cadena `token` y `acme`, cada uno de los cuales implementa las instrucciones`pay()` y `launch_missiles()` respectivamente, acme puede implementarse con una llamada a una función definida en el módulo `token` emitiendo una invocación interprograma:

```rust,ignore
mod acme {
    use token_instruction;

    fn launch_missiles(accounts: &[AccountInfo]) -> Result<()> {
        ...
    }

    fn pay_and_launch_missiles(accounts: &[AccountInfo]) -> Result<()> {
        let alice_pubkey = accounts[1].key;
        let instruction = token_instruction::pay(&alice_pubkey);
        invoke(&instruction, accounts)?;

        launch_missiles(accounts)?;
    }
```

`invoke()` está integrado en el tiempo de ejecución de Solana y es responsable de enrutar la instrucción dada al programa `token` a través del campo `program_id` de la instrucción.

Tenga en cuenta que `invoke` requiere que el llamante pase todas las cuentas requeridas por la instrucción que se está invocando. Esto significa que tanto la cuenta ejecutable (la que coincide con el id de programa de la instrucción) como las cuentas pasadas al procesador de la instrucción.

Antes de invocar `pay()`, el tiempo de ejecución debe asegurarse de que `acme` no ha modificado ninguna cuenta propiedad de `token`. Lo hace aplicando la política del tiempo de ejecución al estado actual de las cuentas en el momento en que `acme` llama a `invoke` frente al estado inicial de las cuentas al comienzo de la instrucción de `acme`. Después de que `pay()` se complete, el tiempo de ejecución debe volver a asegurarse de que `token` no modificó ninguna cuenta propiedad de `acme` aplicando de nuevo la política del tiempo de ejecución, pero esta vez con el ID del programa `token`. Por último, después de que `pay_and_launch_missiles()` finalice, el tiempo de ejecución debe aplicar la política de tiempo de ejecución una vez más, donde normalmente lo haría, pero utilizando todas las variables actualizadas `pre_*`. Si al ejecutar `pay_and_launch_missiles()` hasta `pay()` no se han realizado cambios no válidos en la cuenta `pay()` y no se han realizado cambios no válidos, y al ejecutar desde `pay()` hasta `pay_and_launch_missiles()`, entonces el tiempo de ejecución puede asumir transitivamente que `pay_and_launch_missiles()` en su totalidad no hizo cambios inválidos en la cuenta, y por lo tanto consignar todas estas modificaciones en la cuenta.

### Instrucciones que requieren privilegios

El tiempo de ejecución utiliza los privilegios concedidos al programa que llama para determinar qué privilegios pueden extenderse al que llama. En este contexto, los privilegios se refieren a los firmantes y a las cuentas con capacidad de escritura. Por ejemplo, si la instrucción que la persona que llama está procesando contiene un firmante o una cuenta escribible, entonces la persona que llama puede invocar una instrucción que también contiene ese firmante y/o cuenta escribible.

Esta ampliación de privilegios se basa en el hecho de que los programas son inmutables. En el caso del programa `acme`, el tiempo de ejecución puede tratar con seguridad la firma de la transacción como una firma de una instrucción `token`. Cuando el tiempo de ejecución ve que la instrucción `token` hace referencia a `alice_pubkey`, busca la clave en la instrucción `acme` para ver si esa clave corresponde a una cuenta firmada. En este caso, lo hace y con ello autoriza al programa `token` a modificar la cuenta Alice.

### Programa de cuentas firmadas

Los programas pueden emitir instrucciones que contengan cuentas firmadas que no fueron firmadas en la transacción original utilizando [Direcciones derivadas del programa](#direccionesderivadasdelprograma).

Para firmar una cuenta con direcciones derivadas del programa, un programa puede `invoke_signed()`.

```rust,ignore
        invoke_signed(
            &instruction,
            accounts,
            &[&["First addresses seed"],
              &["Second addresses first seed", "Second addresses second seed"]],
        )?;
```

### Profundidad de llamada

Las Invocaciones interprogramas permiten a los programas invocar directamente a otros programas, pero la profundidad está limitada actualmente a 4.

### Reentrada

La reentrada se limita actualmente a la auto-recurrencia directa limitada a una profundidad fija. Esta restricción evita situaciones en las que un programa podría invocar a otro desde un estado intermedio sin saber que más tarde podría ser llamado de nuevo. La recursividad directa da al programa el control total de su estado en el momento en que es llamado de nuevo.

## Direcciones derivadas del programa

Las direcciones derivadas del programa permiten utilizar la firma generada por el programa cuando hay [llamadas entre programas](#cross-program-invocations).

Utilizando una dirección derivada del programa, se puede dar a un programa la autoridad sobre una cuenta y posteriormente transferir esa autoridad a otro. Esto es posible porque el programa puede actuar como el firmante en la transacción que da autoridad.

Por ejemplo, si dos usuarios quieren hacer una apuesta sobre el resultado de un juego en Solana, cada uno debe transferir los activos de su apuesta a algún intermediario que cumpla su acuerdo. Actualmente, no hay manera de implementar este intermediario como un programa en Solana porque el programa intermediario no puede transferir los activos al ganador.

Esta capacidad es necesaria para muchas aplicaciones DeFi, ya que requieren que los activos se transfieran a un agente de custodia hasta que se produzca algún evento que determine el nuevo propietario.

- Exchanges descentralizados que transfieren activos entre órdenes de compra y venta coincidentes.

- Subastas que transfieren activos al ganador.

- Juegos o mercados de predicción que recogen y redistribuyen premios a los ganadores.

Dirección derivada del programa:

1. Permitir a los programas controlar direcciones específicas, llamadas direcciones del programa, en de tal manera que ningún usuario externo puede generar transacciones válidas con firmas para esas direcciones.

2. Permitir a los programas firmar programáticamente para direcciones de programas que están presentes en instrucciones invocadas a través de [Invocaciones interprogramas](#cross-program-invocations).

Dadas las dos condiciones, los usuarios pueden transferir o asignar de forma segura la autoridad de los activos de la cadena a las direcciones del programa y el programa puede entonces asignar esa autoridad en otro lugar a su discreción.

### Claves privadas para direcciones del programa

Una dirección de programa no se encuentra en la curva ed25519 y, por tanto, no tiene una clave privada válida asociada, por lo que es imposible generar una firma para ella. Aunque no tiene clave privada propia, puede ser utilizado por un programa para emitir una instrucción que incluya la dirección del Programa como firmante.

### Direcciones de programa generadas en base a Hash

Las direcciones de los programas se obtienen de forma determinista a partir de una colección de semillas y un identificador de programa mediante una función hash de 256 bits resistente a la preimagen. La dirección del programa no debe estar en la curva ed25519 para asegurar que no hay una clave privada asociada. Durante la generación aparecerá un error si la dirección se encuentra en la curva. La probabilidad de que esto ocurra es de un 50 % para una determinada colección de semillas y un identificador de programa. Si esto ocurre, se puede utilizar un conjunto diferente de semillas o un bump de semillas (semilla adicional de 8 bits) para encontrar una dirección de programa válida fuera de la curva.

Las direcciones de programa deterministas para los programas siguen una ruta de derivación similar a la de las cuentas creadas con `SystemInstruction::CreateAccountWithSeed` que se implementa con `system_instruction::create_address_with_seed`.

Como referencia, esa implementación es la siguiente:

```rust,ignore
pub fn create_address_with_seed(
    base: &Pubkey,
    seed: &str,
    program_id: &Pubkey,
) -> Result<Pubkey, SystemError> {
    if seed.len() > MAX_ADDRESS_SEED_LEN {
        return Err(SystemError::MaxSeedLengthExceeded);
    }

    Ok(Pubkey::new(
        hashv(&[base.as_ref(), seed.as_ref(), program_id.as_ref()]).as_ref(),
    ))
}
```

Los programas pueden obtener determinadamente cualquier número de direcciones mediante el uso de semillas. Estas semillas pueden identificar simbólicamente cómo se utilizan las direcciones.

De `Pubkey`::

```rust,ignore
/// Generate a derived program address
///     * seeds, symbolic keywords used to derive the key
///     * program_id, program that the address is derived for
pub fn create_program_address(
    seeds: &[&[u8]],
    program_id: &Pubkey,
) -> Result<Pubkey, PubkeyError>
```

### Utilizando las direcciones de los programas

Los clientes pueden utilizar la función `create_program_address` para generar una dirección de destino.

```rust,ignore
// deterministically derive the escrow key
let escrow_pubkey = create_program_address(&[&["escrow"]], &escrow_program_id);

// construct a transfer message using that key
let message = Message::new(vec![
    token_instruction::transfer(&alice_pubkey, &escrow_pubkey, 1),
]);

// process the message which transfer one 1 token to the escrow
client.send_and_confirm_message(&[&alice_keypair], &message);
```

Los programas pueden utilizar la misma función para generar la misma dirección. En la siguiente función el programa emite un `token_instruction::transfer` desde una dirección del programa como si tuviera la clave privada para firmar la transacción.

```rust,ignore
fn transfer_one_token_from_escrow(
    program_id: &Pubkey,
    keyed_accounts: &[KeyedAccount]
) -> Result<()> {

    // User supplies the destination
    let alice_pubkey = keyed_accounts[1].unsigned_key();

    // Deterministically derive the escrow pubkey.
    let escrow_pubkey = create_program_address(&[&["escrow"]], program_id);

    // Create the transfer instruction
    let instruction = token_instruction::transfer(&escrow_pubkey, &alice_pubkey, 1);

    // The runtime deterministically derives the key from the currently
    // executing program ID and the supplied keywords.
    // If the derived address matches a key marked as signed in the instruction
    // then that key is accepted as signed.
    invoke_signed(&instruction,  &[&["escrow"]])?
}
```

### Instrucciones que requieren firmantes

Las direcciones generadas con `create_program_address` son indistinguibles de cualquier otra clave pública. La única manera de que el tiempo de ejecución verifique que la dirección pertenece a un programa es que éste proporcione las semillas utilizadas para generar la dirección.

El tiempo de ejecución llamará internamente a `create_program_address`, y comparará el resultado con las direcciones suministradas en la instrucción.

## Ejemplos

Consulte [Desarrollando con Rust](developing/deployed-programs/../../../deployed-programs/developing-rust.md#examples) y [Desarrollando con C](developing/deployed-programs/../../../deployed-programs/developing-c.md#examples) para ver ejemplos de cómo utilizar las Invocaciones interprogramas.
