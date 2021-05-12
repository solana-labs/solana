---
title: Solana Clusters
---

Solana mantiene varios clusters con diferentes propósitos.

Antes de comenzar asegúrate de tener primero [instalado las herramientas de línea de comandos de Solana](cli/install-solana-cli-tools.md)

Exploradores:

- [http://explorer.solana.com/](https://explorer.solana.com/).
- [http://solanabeach.io/](http://solanabeach.io/).

## Devnet

- La Devnet sirve como campo de juego para cualquiera que quiera seleccionar a Solana durante una unidad de prueba, como usuario, titular de tokens, desarrollador de aplicaciones o validador.
- Los desarrolladores de aplicaciones deberían dirigirse a la Devnet.
- Los validadores Potenciales deberían dirigirse primero a Devnet.
- Diferencias clave entre Devnet y Beta Mainnet:
  - Los tokens de Devnet **no son reales**
  - Devnet incluye un faucet de token para airdrops para pruebas de aplicación
  - Devnet puede estar sujeto a reinicios del ledger
  - Devnet normalmente ejecuta una versión de software más reciente que la Beta Mainnet
- Punto de entrada de Gossip para Devnet: `entrypoint.devnet.solana.com:8001`
- Variable de entorno de las métricas para Devnet:

```bash
export SOLANA_METRICS_CONFIG="host=https://metrics.solana.com:8086,db=devnet,u=scratch_writer,p=topsecret"
```

- URL RPC para Devnet: `https://devnet.solana.com`

##### Ejemplo de configuración de línea de comandos `solana`

```bash
configuración de solana --url https://devnet.solana.com
```

##### Ejemplo de línea de comandos `solana-validator`

```bash
$ solana-validator \
    --identity validator-keypair.json \
    --vote-account vote-account-keypair.json \
    --trusted-validator dv1LfzJvDF7S1fBKpFgKoKXK5yoSosmkAdfbxBo1GqJ \
    --no-untrusted-rpc \
    --ledger ledger \
    --rpc-port 8899 \
    --dynamic-port-range 8000-8010 \
    --entrypoint entrypoint.devnet.solana.com:8001 \
    --expected-genesis-hash EtWTRABZaYq6iMfeYKouRu166VU2xqa1wcaWoxPkrZBG \
    --wal-recovery-mode skip_any_corrupted_record \
    --limit-ledger-size
```

El `--trusted-validator`s está operado por Solana

## Testnet

- La Testnet es el lugar donde probamos las características de la versión reciente en un clúster en un clúster en vivo, especialmente centrado en el rendimiento de la red, la estabilidad y el comportamiento del validador.
- La iniciativa [ Tour de SOL](tour-de-sol.md) se lleva a cabo en Testnet, donde fomentamos el comportamiento malicioso y los ataques a la red para ayudarnos a encontrar y a eliminar bugs o vulnerabilidades de la red.
- Los tokens de la Testnet **no son reales**
- Testnet puede estar sujeto a reinicios del ledger.
- Testnet incluye un faucet de token para airdrops para pruebas de aplicación
- Testnet normalmente ejecuta un lanzamiento de software más reciente que Devnet y Mainnet Beta
- Punto de entrada de Gossip para Testnet: `entrypoint.testnet.solana.com:8001`
- Variable de entorno de las métricas para Testnet:

```bash
export SOLANA_METRICS_CONFIG="host=https://metrics.solana.com:8086,db=tds,u=testnet_write,p=c4fa841aa918bf8274e3e2a44d77568d9861b3ea"
```

- URL RPC para Testnet: `https://testnet.solana.com`

##### Ejemplo de configuración de línea de comandos `solana`

```bash
configuración de solana --url https://testnet.solana.com
```

##### Ejemplo de línea de comandos `solana-validator`

```bash
$ solana-validator \
    --identity validator-keypair.json \
    --vote-account vote-account-keypair.json \
    --trusted-validator 5D1fNXzvv5NjV1ysLjirC4WY92RNsVH18vjmcszZd8on \
    --trusted-validator 7XSY3MrYnK8vq693Rju17bbPkCN3Z7KvvfvJx4kdrsSY \
    --trusted-validator Ft5fbkqNa76vnsjYNwjDZUXoTWpP7VYm3mtsaQckQADN \
    --trusted-validator 9QxCLckBiJc783jnMvXZubK4wH86Eqqvashtrwvcsgkv \
    --no-untrusted-rpc \
    --ledger ledger \
    --rpc-port 8899 \
    --dynamic-port-range 8000-8010 \
    --entrypoint entrypoint.testnet.solana.com:8001 \
    --expected-genesis-hash 4uhcVJyU9pJkvQyS88uRDiswHXSCkY3zQawwpjk2NsNY \
    --wal-recovery-mode skip_any_corrupted_record \
    --limit-ledger-size
```

La identidad del `--trusted-validator`s son:

- `5D1fNXzvv5NjV1ysLjirC4WY92RNsVH18vjmcszZd8on` - Solana Foundation (testnet.solana.com)
- `7XSY3MrYnK8vq693Rju17bbPkCN3Z7KvvfvJx4kdrsSY` - Solana Foundation (Break RPC node)
- `Ft5fbkqNa76vnsjYNwjDZUXoTWpP7VYm3mtsaQckQADN` - Certus One
- `9QxCLckBiJc783jnMvXZubK4wH86Eqqvashtrwvcsgkv` - Algo|Stake

## Beta de Mainnet

Un clúster persistente sin permisos para poseedores de tokens iniciales y socios de lanzamiento. Actualmente, las recompensas y la inflación están deshabilitadas.

- Los tokens emitidos en la Beta Mainnet son **reales** SOL
- Si has pagado dinero para comprar/recibir tokens, como por ejemplo a través de nuestra subasta CoinList, estos tokens se transferirán en Mainnet Beta.
  - Nota: Si está utilizando una billetera que no es de línea de comandos, como [Solflare](wallet-guide/solflare.md), la billetera siempre se conectará a Mainnet Beta.
- Punto de entrada de Gossip para Mainnet Beta: `entrypoint.mainnet-beta.solana.com:8001`
- Variable de entorno de las métricas para Mainnet Beta:

```bash
export SOLANA_METRICS_CONFIG="host=https://metrics.solana.com:8086,db=mainnet-beta,u=mainnet-beta_write,p=password"
```

- URL RPC para Mainnet Beta: `https://api.mainnet-beta.solana.com`

##### Ejemplo de configuración de línea de comandos `solana`

```bash
configuración de solana --url https://api.mainnet-beta.solana.com
```

##### Ejemplo de línea de comandos `solana-validator`

```bash
$ solana-validator \
    --identity ~/validator-keypair.json \
    --vote-account ~/vote-account-keypair.json \
    --trusted-validator 7Np41oeYqPefeNQEHSv1UDhYrehxin3NStELsSKCT4K2 \
    --trusted-validator GdnSyH3YtwcxFvQrVVJMm1JhTS4QVX7MFsX56uJLUfiZ \
    --trusted-validator DE1bawNcRJB9rVm3buyMVfr8mBEoyyu73NBovf2oXJsJ \
    --trusted-validator CakcnaRDHka2gXyfbEd2d3xsvkJkqsLw2akB3zsN1D2S \
    --no-untrusted-rpc \
    --ledger ledger \
    --rpc-port 8899 \
    --private-rpc \
    --dynamic-port-range 8000-8010 \
    --entrypoint entrypoint.mainnet-beta.solana.com:8001 \
    --entrypoint entrypoint2.mainnet-beta.solana.com:8001 \
    --entrypoint entrypoint3.mainnet-beta.solana.com:8001 \
    --entrypoint entrypoint4.mainnet-beta.solana.com:8001 \
    --entrypoint entrypoint5.mainnet-beta.solana.com:8001 \
    --expected-genesis-hash 5eykt4UsFv8P8NJdTREpY1vzqKqZKvdpKuc147dw2N9d \
    --wal-recovery-mode skip_any_corrupted_record \
    --limit-ledger-size
```

Los cuatro `--trusted-validator`s son operados por Solana
