---
title: Monitoreando un Validador
---

## Comprobar Gossip

Confirma que la dirección IP y la **pubkey de identidad** de tu validador son visibles en la red Gossip ejecutando:

```bash
solana-gossip spy --entrypoint devnet.solana.com:8001
```

## Compruebe su saldo

El saldo de tu cuenta debe disminuir por el monto de las comisiones de transacción a medida que tu validador envíe votos, y aumentar después de servir como líder. Pasar el `--lamports` son para observar con más detalle:

```bash
saldo de solana --lamports
```

## Comprobar actividad de voto

El comando `solana vote-account` muestra la actividad de votación reciente de tu validador:

```bash
cuenta de voto ~/vote-account-keypair.json
```

## Obtener información de Cluster

Hay varios endpoints JSON-RPC útiles para monitorear su validador en el clúster, así como la salud del clúster:

```bash
# Similar to solana-gossip, you should see your validator in the list of cluster nodes
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getClusterNodes"}' http://devnet.solana.com
# If your validator is properly voting, it should appear in the list of `current` vote accounts. Si se activa, `stake` debería ser > 0
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getVoteAccounts"}' http://devnet.solana. om
# Devuelve el actual leader schedule
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2. ","id":1, "method":"getLeaderSchedule"}' http://devnet.solana.com
# Devuelve información sobre la época actual. slotIndex debe progresar en llamadas posteriores.
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getEpochInfo"}' http://devnet.solana.com
```
