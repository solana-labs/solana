---
title: Comparar un clúster
---

El repositorio de git de Solana contiene todos los scripts que puede necesitar para crear su propia red de prueba local. Dependiendo de lo que esté buscando lograr, es posible que desee ejecutar una variación diferente, ya que la red de prueba multinodo completa y con rendimiento mejorado es considerablemente más compleja de configurar que un nodo de prueba de un solo nodo solo Rust. Si está buscando desarrollar funciones de alto nivel, como experimentar con contratos inteligentes, ahórrese algunos dolores de cabeza de configuración y apéguese a la demostración de un solo nodo solo para Rust. Si está optimizando el rendimiento de la canalización de transacciones, considere la demostración mejorada de un solo nodo. Si está haciendo un trabajo de consenso, necesitará al menos una demostración multinodo solo de Rust. Si desea reproducir nuestras métricas de TPS, ejecute la demostración multinodo mejorada.

Para las cuatro variaciones, necesitaría la última cadena de herramientas de Rust y el código fuente de Solana:

Primero, configure los paquetes Rust, Cargo y del sistema como se describe en Solana. [LEEME](https://github.com/solana-labs/solana#1-install-rustc-cargo-and-rustfmt)

Ahora revisa el código de github:

```bash
clon de git https://github.com/solana-labs/solana.git
cd solana
```

El código de demostración a veces se rompe entre lanzamientos a medida que agregamos nuevas funciones de bajo nivel, por lo que si es la primera vez que ejecuta la demostración, mejorará sus probabilidades de éxito si consulta el [ última versión ](https: // github.com/solana-labs/solana/releases) antes de continuar:

```bash
TAG=$(git describe --tags $(git rev-list --tags --max-count=1))
git checkout $TAG
```

### Valores de Configuración

Asegúrese de que se creen programas importantes, como el programa de votación, antes de que se inicien los nodos. Tenga en cuenta que estamos usando la versión de lanzamiento aquí para un buen rendimiento. Si desea la compilación de depuración, use solo ` cargo build ` y omita la parte ` NDEBUG = 1 ` del comando.

```bash
construcción de carga - liberación
```

La red se inicializa con un libro de génesis generado mediante la ejecución de la siguiente secuencia de comandos.

```bash
NDEBUG=1 ./multinode-demo/setup.sh
```

### Grifo

Para que los validadores y clientes funcionen, necesitaremos girar un grifo para dar algunos tokens de prueba. El Grifo entrega "airdrops" al estilo de Milton Friedman \ (tokens gratuitos para los clientes solicitantes \) para ser utilizados en transacciones de prueba.

Inicie el grifo con:

```bash
NDEBUG=1 ./multinode-demo/faucet.sh
```

### Testnet de nodo único

Antes de iniciar un validador, asegúrese de conocer la dirección IP de la máquina que desea que sea el validador de arranque para la demostración y asegúrese de que los puertos udp 8000-10000 estén abiertos en todas las máquinas con las que desea realizar la prueba.

Ahora inicie el validador de arranque en un shell separado:

```bash
NDEBUG=1 ./multinode-demo/bootstrap-validator.sh
```

Espere unos segundos para que el servidor se inicialice. Se imprimirá "líder listo..." cuando esté listo para recibir transacciones. El líder solicitará algunas fichas del grifo si no tiene ninguna. No es necesario que el grifo esté abierto para los siguientes arranques de líder.

### Testnet multinodo

Para ejecutar una red de prueba multinodo, después de iniciar un nodo líder, active algunos validadores adicionales en shells separados:

```bash
NDEBUG=1 ./multinode-demo/validator-x.sh
```

Para ejecutar un validador de rendimiento mejorado en Linux, [ CUDA 10.0 ](https://developer.nvidia.com/cuda-downloads) debe estar instalado en su sistema:

```bash
./fetch-perf-libs.sh
NDEBUG=1 SOLANA_CUDA=1 ./multinode-demo/bootstrap-validator.sh
NDEBUG=1 SOLANA_CUDA=1 ./multinode-demo/validator.sh
```

### Demostración del cliente de Testnet

Ahora que su testnet de nodo único o multinodo está en funcionamiento, ¡Enviemos algunas transacciones!

En un shell separado inicie el cliente:

```bash
NDEBUG=1 ./multinode-demo/bench-tps.sh # se ejecuta contra localhost por defecto
```

¿Qué acaba de suceder? La demostración del cliente activa varios hilos para enviar 500.000 transacciones a la red de prueba lo más rápido posible. El cliente realiza pings de la red de pruebas periódicamente para ver cuántas transacciones procesó en ese tiempo. Tenga en cuenta que la demostración inunda intencionalmente la red con paquetes UDP, de tal manera que la red casi con toda seguridad dejará un montón de ellos. Esto asegura que la red de pruebas tiene la oportunidad de alcanzar 710k TPS. La demostración del cliente se completa después de que se haya convencido de que la red de pruebas no procesará ninguna transacción adicional. Deberías ver varias mediciones TPS impresas en la pantalla. En la variación multinodo, también verá las mediciones de TPS para cada nodo validador.

### Depuración de testnet

Hay algunos mensajes de depuración útiles en el código, puedes activarlos por módulo y por nivel. Antes de ejecutar un líder o validador establezca la variable de entorno normal RUST_LOG.

Por ejemplo

- Para habilitar `información` en todas partes y `depuración` sólo en el módulo solana::banking_stay:

  ```bash
export RUST_LOG=solana=info,solana::banking_stage=debug
  ```

- Para habilitar el registro de programas BPF:

  ```bash
export RUST_LOG=solana_bpf_loader=trace
  ```

Generalmente usamos ` debug ` para mensajes de depuración poco frecuentes, ` trace ` para mensajes potencialmente frecuentes y ` info ` para registros relacionados con el rendimiento.

También puede adjuntar un proceso en ejecución con GDB. El proceso del líder se llama _ solana-validator _:

```bash
sudo gdb
attach <PID>
set logging on
thread apply all bt
```

Esto volcará todos los rastros de la pila de subprocesos en gdb.txt

## Testnet del desarrollador

En este ejemplo el cliente se conecta a nuestra red pública de pruebas. Para ejecutar validadores en la red de pruebas necesitaría abrir puertos udp `8000-10000`.

```bash
NDEBUG=1 ./multinode-demo/bench-tps.sh --entrypoint devnet.solana.com:8001 --faucet devnet.solana.com:9900 --duration 60 --tx_count 50
```

Puedes observar los efectos de las transacciones de tus clientes en nuestro panel de [métricas](https://metrics.solana.com:3000/d/monitor/cluster-telemetry?var-testnet=devnet)
