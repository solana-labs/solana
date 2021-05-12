---
title: JSON RPC API
---

Solana nodes accept HTTP requests using the [JSON-RPC 2.0](https://www.jsonrpc.org/specification) specification.

To interact with a Solana node inside a JavaScript application, use the [solana-web3.js](https://github.com/solana-labs/solana-web3.js) library, which gives a convenient interface for the RPC methods.

## RPC HTTP Endpoint

**Default port:** 8899 eg. [http://localhost:8899](http://localhost:8899), [http://192.168.1.88:8899](http://192.168.1.88:8899)

## RPC PubSub WebSocket Endpoint

**Default port:** 8900 eg. ws://localhost:8900, [http://192.168.1.88:8900](http://192.168.1.88:8900)

## Methods

- [getAccountInfo](jsonrpc-api.md#getaccountinfo)
- [getBalance](jsonrpc-api.md#getbalance)
- [getBlockCommitment](jsonrpc-api.md#getblockcommitment)
- [getBlockTime](jsonrpc-api.md#getblocktime)
- [getClusterNodes](jsonrpc-api.md#getclusternodes)
- [getConfirmedBlock](jsonrpc-api.md#getconfirmedblock)
- [getConfirmedBlocks](jsonrpc-api.md#getconfirmedblocks)
- [getConfirmedBlocksWithLimit](jsonrpc-api.md#getconfirmedblockswithlimit)
- [getConfirmedSignaturesForAddress](jsonrpc-api.md#getconfirmedsignaturesforaddress)
- [getConfirmedSignaturesForAddress2](jsonrpc-api.md#getconfirmedsignaturesforaddress2)
- [getConfirmedTransaction](jsonrpc-api.md#getconfirmedtransaction)
- [getEpochInfo](jsonrpc-api.md#getepochinfo)
- [getEpochSchedule](jsonrpc-api.md#getepochschedule)
- [getFeeCalculatorForBlockhash](jsonrpc-api.md#getfeecalculatorforblockhash)
- [getFeeRateGovernor](jsonrpc-api.md#getfeerategovernor)
- [getFees](jsonrpc-api.md#getfees)
- [getFirstAvailableBlock](jsonrpc-api.md#getfirstavailableblock)
- [getGenesisHash](jsonrpc-api.md#getgenesishash)
- [getHealth](jsonrpc-api.md#gethealth)
- [getIdentity](jsonrpc-api.md#getidentity)
- [getInflationGovernor](jsonrpc-api.md#getinflationgovernor)
- [getInflationRate](jsonrpc-api.md#getinflationrate)
- [getLargestAccounts](jsonrpc-api.md#getlargestaccounts)
- [getLeaderSchedule](jsonrpc-api.md#getleaderschedule)
- [getMinimumBalanceForRentExemption](jsonrpc-api.md#getminimumbalanceforrentexemption)
- [getMultipleAccounts](jsonrpc-api.md#getmultipleaccounts)
- [getProgramAccounts](jsonrpc-api.md#getprogramaccounts)
- [getRecentBlockhash](jsonrpc-api.md#getrecentblockhash)
- [getRecentPerformanceSamples](jsonrpc-api.md#getrecentperformancesamples)
- [getSignatureStatuses](jsonrpc-api.md#getsignaturestatuses)
- [getSlot](jsonrpc-api.md#getslot)
- [getSlotLeader](jsonrpc-api.md#getslotleader)
- [getStakeActivation](jsonrpc-api.md#getstakeactivation)
- [getSupply](jsonrpc-api.md#getsupply)
- [getTransactionCount](jsonrpc-api.md#gettransactioncount)
- [getVersion](jsonrpc-api.md#getversion)
- [getVoteAccounts](jsonrpc-api.md#getvoteaccounts)
- [minimumLedgerSlot](jsonrpc-api.md#minimumledgerslot)
- [Airdrop](jsonrpc-api.md#requestairdrop)
- [enviarTransação](jsonrpc-api.md#sendtransaction)
- [simularTransação](jsonrpc-api.md#simulatetransaction)
- [seterFiltro](jsonrpc-api.md#setlogfilter)
- [validatorExit](jsonrpc-api.md#validatorexit)
- [Websocket de assinatura](jsonrpc-api.md#subscription-websocket)
  - [Inscrever](jsonrpc-api.md#accountsubscribe)
  - [Desinscrever](jsonrpc-api.md#accountunsubscribe)
  - [Inscrever-se](jsonrpc-api.md#logssubscribe)
  - [Cancelar inscrição](jsonrpc-api.md#logsunsubscribe)
  - [programSubscribe](jsonrpc-api.md#programsubscribe)
  - [programUnsubscribe](jsonrpc-api.md#programunsubscribe)
  - [Assinar](jsonrpc-api.md#signaturesubscribe)
  - [AssinarDesinscrever](jsonrpc-api.md#signatureunsubscribe)
  - [Inscrever-se](jsonrpc-api.md#slotsubscribe)
  - [Desinscrever](jsonrpc-api.md#slotunsubscribe)

## Métodos instáveis

Métodos instáveis podem ver mudanças nas versões de patch e não podem ser suportados perpetuamente.

- [Saldo](jsonrpc-api.md#gettokenaccountbalance)
- [getTokenAccountsByDelegate](jsonrpc-api.md#gettokenaccountsbydelegate)
- [getTokenAccountsByOwner](jsonrpc-api.md#gettokenaccountsbyowner)
- [Contas](jsonrpc-api.md#gettokenlargestaccounts)
- [Fornecimento](jsonrpc-api.md#gettokensupply)

## Formatação do pedido

Para fazer uma solicitação JSON-RPC, envie uma solicitação HTTP POST com um cabeçalho `Content-Type:
application/json`. Os dados de solicitação JSON devem conter 4 campos:

- `jsonrpc: <string>`, defina como `"2.0"`
- `id: <number>`, um inteiro de identificação único gerado pelo cliente
- `método: <string>`, uma string que contém o método a ser chamado
- `parâmetros: <array>`, uma matriz JSON de valores de parâmetros ordenados

Exemplo usando o curl:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getBalance",
    "params": [
      "83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri"
    ]
  }
'
```

A saída da resposta será um objeto JSON com os seguintes campos:

- `jsonrpc: <string>`, correspondente à especificação do pedido
- `ID: <number>`, correspondente ao identificador de solicitação
- `resultado: <array|number|object|string>`, dados solicitados ou confirmação do sucesso

Pedidos podem ser enviados em lotes enviando uma matriz de objetos de solicitação JSON-RPC como dados para um único POST.

## Definições

- Hash: Um hash SHA-256 de um pedaço de dados.
- Pubkey: A chave pública de um par de chaves Ed25519.
- Transação: Uma lista de instruções Solana assinadas por um keypair de cliente para autorizar essas ações.
- Assinatura: Uma assinatura Ed25519 dos dados payload da transação, incluindo instruções. Isto pode ser usado para identificar transações.

## Configurando compromisso de estado

Para verificações de pré-voo e processamento de transação, Solana nós escolhe qual estado do banco para consultar, com base em um requisito de compromisso definido pelo cliente. O compromisso descreve o quão finalizado um bloco é naquele momento  Quando consultar o estado da contabilidade é recomendado usar níveis mais baixos de compromisso para relatar o progresso e níveis mais altos para garantir que o estado não será recuado.

Em ordem decrescente de compromisso (mais finalizado para menos finalizado), os clientes podem especificar:

- `"max"` - the node will query the most recent block confirmed by supermajority of the cluster as having reached maximum lockout, meaning the cluster has recognized this block as finalized
- `"root"` - o nó irá consultar o bloco mais recente de ter alcançado o máximo bloqueio neste nó, significando que o nó reconheceu este bloco como finalizado
- `"singleGossip"` - o nó irá consultar o bloco mais recente que foi votado pela supermaioria do cluster.
  - Ela incorpora votos de fofocas e de repetição.
  - Não conta votos nos descendentes de um bloco, apenas votos directos nesse bloco.
  - Este nível de confirmação também apoia as garantias de "confirmação otimista" na liberação 1.3 e adiante.
- `"recente"` - o nó irá consultar seu bloco mais recente.  Observe que o bloco pode não estar completo.

Para processar muitas transações dependentes em série, é recomendado usar o compromisso `"singleGossip"` </code>, que balança a velocidade com segurança de reversão. Para a segurança total, é recomendado usar o compromisso`"máximo"` de compromisso.

#### Exemplo

The commitment parameter should be included as the last element in the `params` array:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getBalance",
    "params": [
      "83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri",
      {
        "commitment": "max"
      }
    ]
  }
'
```

#### Default:

If commitment configuration is not provided, the node will default to `"max"` commitment

Only methods that query bank state accept the commitment parameter. They are indicated in the API Reference below.

#### RpcResponse Structure

Many methods that take a commitment parameter return an RpcResponse JSON object comprised of two parts:

- `context` : An RpcResponseContext JSON structure including a `slot` field at which the operation was evaluated.
- `value` : The value returned by the operation itself.

## Health Check

Although not a JSON RPC API, a `GET /health` at the RPC HTTP Endpoint provides a health-check mechanism for use by load balancers or other network infrastructure. This request will always return a HTTP 200 OK response with a body of "ok" or "behind" based on the following conditions:

1. If one or more `--trusted-validator` arguments are provided to `solana-validator`, "ok" is returned when the node has within `HEALTH_CHECK_SLOT_DISTANCE` slots of the highest trusted validator, otherwise "behind" is returned.
2. "ok" is always returned if no trusted validators are provided.

## JSON RPC API Reference

### getAccountInfo

Returns all information associated with the account of provided Pubkey

#### Parameters:

- `<string>` - Pubkey of account to query, as base-58 encoded string
- `<object>` - (optional) Configuration object containing the following optional fields:
  - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)
  - `encoding: <string>` - encoding for Account data, either "base58" (*slow*), "base64", "base64+zstd", or "jsonParsed". "base58" is limited to Account data of less than 128 bytes. "base64" will return base64 encoded data for Account data of any size. "base64+zstd" compresses the Account data using [Zstandard](https://facebook.github.io/zstd/) and base64-encodes the result. "jsonParsed" encoding attempts to use program-specific state parsers to return more human-readable and explicit account state data. If "jsonParsed" is requested but a parser cannot be found, the field falls back to "base64" encoding, detectable when the `data` field is type `<string>`.
  - (optional) `dataSlice: <object>` - limit the returned account data using the provided `offset: <usize>` and `length: <usize>` fields; only available for "base58", "base64" or "base64+zstd" encodings.

#### Results:

The result will be an RpcResponse JSON object with `value` equal to:

- `<null>` - if the requested account doesn't exist
- `<object>` - otherwise, a JSON object containing:
  - `lamports: <u64>`, number of lamports assigned to this account, as a u64
  - `owner: <string>`, base-58 encoded Pubkey of the program this account has been assigned to
  - `data: <[string, encoding]|object>`, data associated with the account, either as encoded binary data or JSON format `{<program>: <state>}`, depending on encoding parameter
  - `executable: <bool>`, boolean indicating if the account contains a program \(and is strictly read-only\)
  - `rentEpoch: <u64>`, the epoch at which this account will next owe rent, as u64

#### Example:

Solicitação:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getAccountInfo",
    "params": [
      "vines1vzrYbzLMRdu58ou5XTby4qAqVRLmqo36NKPTg",
      {

```
Resposta:
```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1
    },
    "value": {
      "data": [
        "11116bv5nS2h3y12kD1yUKeMZvGcKLSjQgX6BeV7u1FrjeJcKfsHRTPuR3oZ1EioKtYGiYxpxMG5vpbZLsbcBYBEmZZcMKaSoGx9JZeAuWf",
        "base58"
      ],
      "executable": false,
      "lamports": 1000000000,
      "owner": "11111111111111111111111111111111",
      "rentEpoch": 2
    }
  },
  "id": 1
}
```

#### Exemplo:
Pedido:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2. ",
    "id": 1,
    "method": "getAccountInfo",
    "params": [
      "4fYNw3dojWmQ4dXtSGE9epjRGy9pFSx62YypT7avPYvA",
      {
        "encoding": "jsonsed"
      }
    ]
  }
'
```
Resposta:
```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1
    },
    "value": {
      "data": {
        "nonce": {
          "initialized": {
            "authority": "Bbqg1M4YVVfbhEzwA9SpC9FhsaG83YMTYoR4a8oTDLX",
            "blockhash": "3xLP3jK6dVJwpeGeTDYTwdDK3TKchUf1gYYGHa4sF3XJ",
            "feeCalculator": {
              "lamportsPerSignature": 5000
            }
          }
        }
      },
      "executable": false,
      "lamports": 1000000000,
      "owner": "11111111111111111111111111111111",
      "rentEpoch": 2
    }
  },
  "id": 1
}
```

### Saldo

Devolve o saldo da conta da Pubkey fornecida

#### Parâmetros:

- `<string>` - Publique a chave da conta a ser consultada, como string codificada com base-58
- `<object>` - (opcional) [Compromisso](jsonrpc-api.md#configuring-state-commitment)

#### Resultados:

- `RpcResponse<u64>` - Objeto RpcResponse JSON com o valor `` do campo definido para o equilíbrio

#### Exemplo:

Solicitação:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '  {    "jsonrpc": "2.0",    "id": 1,    "method": "getBalance",    "params": [      "83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri"    ]
  }
'
```

Resultados:
```json
{"jsonrpc":"2.0","resultado":{"context":{"slot":1},"value":0},"id":1}
```

### Compromisso

Retorna compromisso para um bloco específico

#### Parâmetros:

- `<u64>` - bloco, identificado pelo Slot

#### Resultados:

O resultado será um objeto JSON contendo:

- `commitment` - compromisso, compreendendo também:
  - `<null>` - Bloco desconhecido
  - `<array>` - compromisso, array de u64 inteiros registram a quantidade da participação do cluster em lâmpadas que votou no bloco a cada profundidade de 0 a `MAX_LOCKOUT_HISTORY` + 1
- `TotalStake` - total acionamento ativo, em lâmpadas, do período atual

#### Exemplo:

Solicitação:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getBlockCommitment","params":[5]}
'
```

Resultados:
```json
{
  "jsonrpc":"2.0",
  "result":{
    "commitment":[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,10,32],
    "totalStake": 42
  },
  "id":1
}
```

### getBlockTime

Retorna o tempo de produção estimado de um bloco confirmado.

Cada validador reporta seu tempo UTC ao registro em um intervalo regular por adicionando um carimbo de hora a um voto para um bloco específico. Um tempo solicitado é calculado a partir da média ponderada das datas de votação em um conjunto de blocos recentes registrados no ledger.

Nós que estão inicializando a partir de snapshot ou limitando o tamanho do livro razão (removendo antigos slots) irão retornar horários nulos para blocos abaixo da sua menor raiz + `TIMESTAMP_SLOT_RANGE`. Os usuários interessados em ter esses dados históricos devem consultar um nó que é construído a partir de genesis e mantém o livro razão inteiro.

#### Parâmetros:

- `<u64>` - bloco, identificado pelo Slot

#### Resultados:

* `<i64>` - tempo estimado de produção, como horário Unix (segundos desde o período Unix)
* `<null>` - timestamp não está disponível para este bloco

#### Exemplo:

Solicitação:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getBlockCommitment","params":[5]}
'
```

Result:
```json
{"jsonrpc":"2.0","result":1574721591,"id":1}
```

### getClusterNodes

Returns information about all the nodes participating in the cluster

#### Parameters:

None

#### Results:

The result field will be an array of JSON objects, each with the following sub fields:

- `pubkey: <string>` - Node public key, as base-58 encoded string
- `gossip: <string>` - Gossip network address for the node
- `tpu: <string>` - TPU network address for the node
- `rpc: <string>|null` - JSON RPC network address for the node, or `null` if the JSON RPC service is not enabled
- `version: <string>|null` - The software version of the node, or `null` if the version information is not available

#### Example:

Request:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getClusterNodes"}
'
```

Result:
```json
{
  "jsonrpc": "2.0",
  "result": [
    {
      "gossip": "10.239.6.48:8001",
      "pubkey": "9QzsJf7LPLj8GkXbYT3LFDKqsj2hHG7TA3xinJHu8epQ",
      "rpc": "10.239.6.48:8899",
      "tpu": "10.239.6.48:8856",
      "version": "1.0.0 c375ce1f"
    }
  ],
  "id": 1
}
```

### getConfirmedBlock

Returns identity and transaction information about a confirmed block in the ledger

#### Parameters:

- `<u64>` - slot, as u64 integer
- `<string>` - encoding for each returned Transaction, either "json", "jsonParsed", "base58" (*slow*), "base64". If parameter not provided, the default encoding is "json". "jsonParsed" encoding attempts to use program-specific instruction parsers to return more human-readable and explicit data in the `transaction.message.instructions` list. If "jsonParsed" is requested but a parser cannot be found, the instruction falls back to regular JSON encoding (`accounts`, `data`, and `programIdIndex` fields).

#### Results:

The result field will be an object with the following fields:

- `<null>` - if specified block is not confirmed
- `<object>` - if block is confirmed, an object with the following fields:
  - `blockhash: <string>` - the blockhash of this block, as base-58 encoded string
  - `previousBlockhash: <string>` - the blockhash of this block's parent, as base-58 encoded string; if the parent block is not available due to ledger cleanup, this field will return "11111111111111111111111111111111"
  - `parentSlot: <u64>` - the slot index of this block's parent
  - `transactions: <array>` - an array of JSON objects containing:
    - `transaction: <object|[string,encoding]>` - [Transaction](#transaction-structure) object, either in JSON format or encoded binary data, depending on encoding parameter
    - `meta: <object>` - transaction status metadata object, containing `null` or:
      - `err: <object | null>` - Error if transaction failed, null if transaction succeeded. [TransactionError definitions](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
      - `fee: <u64>` - fee this transaction was charged, as u64 integer
      - `preBalances: <array>` - array of u64 account balances from before the transaction was processed
      - `postBalances: <array>` - array of u64 account balances after the transaction was processed
      - `innerInstructions: <array|undefined>` - List of [inner instructions](#inner-instructions-structure) or omitted if inner instruction recording was not yet enabled during this transaction
      - `logMessages: <array>` - array of string log messages or omitted if log message recording was not yet enabled during this transaction
      - DEPRECATED: `status: <object>` - Transaction status
        - `"Ok": <null>` - Transaction was successful
        - `"Err": <ERR>` - Transaction failed with TransactionError
  - `rewards: <array>` - an array of JSON objects containing:
    - `pubkey: <string>` - The public key, as base-58 encoded string, of the account that received the reward
    - `lamports: <i64>`- number of reward lamports credited or debited by the account, as a i64
    - `postBalance: <u64>` - account balance in lamports after the reward was applied
    - `Tipo de recompensa: <string|undefined>` - tipo de recompensa: "taxa", "rent", "voto", "staking"
  - `<i64 | null>` - tempo estimado de produção, como horário Unix (segundos desde o período Unix). null se não disponível

#### Exemplo:

Solicitação:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getBlockCommitment","params":[5]}
'
```

Resultados:
```json
{
  "jsonrpc": "2.0",
  "result": {
    "blockTime": null,
    "blockhash": "3Eq21vXNB5s86c62bVuUfTeaMif1N2kUqRPBmGRJhyTA",
    "parentSlot": 429,
    "previousBlockhash": "mfcyqEXB3DnHXki6KjjmZck6YjmZLvpAByy2fj4nh6B",
    "rewards": [],
    "transactions": [
      {
        "meta": {
          "err": null,
          "fee": 5000,
          "innerInstructions": [],
          "logMessages": [],
          "postBalances": [
            499998932500,
            26858640,
            1,
            1,
            1
          ],
          "preBalances": [
            499998937500,
            26858640,
            1,
            1,
            1
          ],
          "status": {
            "Ok": null
          }
        },
        "transaction": {
          "message": {
            "accountKeys": [
              "3UVYmECPPMZSCqWKfENfuoTv51fTDTWicX9xmBD2euKe",
              "AjozzgE83A3x1sHNUR64hfH7zaEBWeMaFuAN9kQgujrc",
              "SysvarS1otHashes111111111111111111111111111",
              "SysvarC1ock11111111111111111111111111111111",
              "Vote111111111111111111111111111111111111111"
            ],
            "header": {
              "numReadonlySignedAccounts": 0,
              "numReadonlyUnsignedAccounts": 3,
              "numRequiredSignatures": 1
            },
            "instructions": [
              {
                "accounts": [
                  1,
                  2,
                  3,
                  0
                ],
                "data": "37u9WtQpcm6ULa3WRQHmj49EPs4if7o9f1jSRVZpm2dvihR9C8jY4NqEwXUbLwx15HBSNcP1",
                "programIdIndex": 4
              }
            ],
            "recentBlockhash": "mfcyqEXB3DnHXki6KjjmZck6YjmZLvpAByy2fj4nh6B"
          },
          "signatures": [
            "2nBhEBYYvfaAe16UMNqRHre4YNSskvuYgx3M6E4JP1oDYvZEJHvoPzyUidNgNX5r9sTyN1J9UxtbCXy2rqYcuyuv"
          ]
        }
      }
    ]
  },
  "id": 1
}
```

#### Exemplo:
Solicitação:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getBlockCommitment","params":[5]}
'
```

Resultados:
```json
{
  "jsonrpc": "2. ",
  "result": {
    "blockTime": nulo,
    "blockhash": "3Eq21vXNB5s86c62bVuUfTeaMif1N2kUqRBmGRJhyTA",
    "parentSlot": 429,
    "previousBlockhash": "mfcyqEXB3DnHXki6KjjmZck6YjmZLvpAByy2fj4nh6B",
    "recompensas": [],
    "transações": [
      {
        "meta": {
          "err": null,
          "taxa": 5000,
          "innerInstructions": [],
          "logMessages": [],
          "postBalances": [
            499998932500,
            26858640,
            1,
            1
            1
          ],
          "preSaldos": [
            499998937500,
            26858640,
            1,
            1,
            1
          ],
          "status": {
            "Ok": nulo
          }
        },
        "transacção": [
          "AVj7dxHlQ9IrvdYVIjuiRFs1jLaDMHixgrv+qtHBwz51L4/ImLZhszwiyEJDIp7xeBSpm/TX5B7mYzxa+fPOMw0BAAMFJVqLw+hJYheizSoYlLm53KzgT82cDVmazarqQKG2GQsLgiqktA+a+FDR4/7xnDX7rsusMwryYVUdiz1B1Q3KzgLm53KzgT82cDVmazarqQQKQKQQKG2GQsLgiqktLgiqkt+F7xA
          "base64"
        ]
      }
    ]
  },
  "id": 1
}
```

#### Estrutura de Transação

As transações são bem diferentes das em outras blockchains. Não se esqueça de revisar [Anatomy de uma transação](developing/programming-model/transactions.md) para aprender sobre transações na Solana.

A estrutura JSON de uma transação é definida da seguinte forma:

- `assinaturas: <array[string]>` - Uma lista de assinaturas codificadas base-58 aplicadas à transação. A lista é sempre de comprimento `message.header.numRequiredSignatures` e não está vazia. A assinatura no índice `i` corresponde à chave pública no índice `i` em `message.account_keys`. O primeiro é usado como [ID de transação](../../terminology.md#transaction-id).
- `mensagem: <object>` - Define o conteúdo da transação.
  - `Keys: <array[string]>` - Lista de chaves públicas codificadas por base-58 usadas pela transação, incluindo pelas instruções e assinaturas. As primeiras `message.header.numRequiredSignatures` chaves públicas devem assinar a transação.
  - `cabeçalho: <object>` - Detalhes os tipos de conta e assinaturas necessários pela transação.
    - `num.Assinaturas: <number>` - O número total de assinaturas necessárias para tornar a transação válida. As assinaturas devem corresponder às primeiras `numRequiredSignatures` of `message.account_keys`.
    - `numReadonlySignedAccounts: <number>` - As últimas `numonlyAssinadasContas` das chaves assinadas são contas somente leitura. Programas podem processar múltiplas transações que carregam contas somente de leitura dentro de uma única entrada PoH, mas não tem permissão para acessar lâmpadas de crédito ou débito ou modificar os dados da conta. Transações direcionadas à mesma conta de leitura-escrita são avaliadas sequencialmente.
    - `numReadonlySignedAccounts: <number>` - As últimas `numonlyAssinadasContas` das chaves assinadas são contas somente leitura.
  - `recentBlockhash: <string>` - Um hash codificado base-58 de um bloco recente no livro-razão usado para evitar a duplicação de transações e dar vida às transações.
  - `instruções: <array[object]>` - Lista de instruções do programa que serão executadas em sequência e confirmadas em uma transação atômica se todos forem bem-sucedidas.
    - `programIdIndex: <number>` - Índice da `matriz message.accountKeys` indicando a conta do programa que executa esta instrução.
    - `Contas: <array[number]>` - Lista de índices ordenados no `array message.accountKeys` indicando quais contas devem passar para o programa.
    - `data: <string>` - Os dados de entrada do programa codificados em uma string base-58.

#### Estrutura das Instruções Internas

O tempo de execução Solana registra as instruções de multi programas que são invocadas durante o processamento de transações e as torna disponíveis para maior transparência do que foi executado na cadeia por instrução de transação. As instruções invocadas são agrupadas pela instrução da transação de origem e estão listadas em ordem de processamento.

A estrutura JSON de instruções internas é definida como uma lista de objetos na seguinte estrutura:

- `índice: número` - Índice da instrução da transação a partir da qual as instruções internas se originaram
- `Instruções especiais: <array[object]>` - Lista ordenada de instruções do programa interno que foram chamadas durante uma única instrução da transação.
  - `programIdIndex: <number>` - Índice da `matriz message.accountKeys` indicando a conta do programa que executa esta instrução.
  - `Contas: <array[number]>` - Lista de índices ordenados no `array message.accountKeys` indicando quais contas devem passar para o programa.
  - `data: <string>` - Os dados de entrada do programa codificados em uma string base-58.

### getConfirmedBlocks

Retorna uma lista de blocos confirmados entre dois slots

#### Parâmetros:

- `<u64>` - start_slot, como u64 inteiro
- `<u64>` - start_slot, como u64 inteiro

#### Results:

The result field will be an array of u64 integers listing confirmed blocks between `start_slot` and either `end_slot`, if provided, or latest confirmed block, inclusive.  Max range allowed is 500,000 slots.


#### Example:

Request:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc": "2.0","id":1,"method":"getConfirmedBlocks","params":[5, 10]}
'
```

Result:
```json
{"jsonrpc":"2.0","result":[5,6,7,8,9,10],"id":1}
```

### getConfirmedBlocksWithLimit

Returns a list of confirmed blocks starting at the given slot

#### Parameters:

- `<u64>` - start_slot, as u64 integer
- `<u64>` - limit, as u64 integer

#### Results:

The result field will be an array of u64 integers listing confirmed blocks starting at `start_slot` for up to `limit` blocks, inclusive.

#### Example:

Request:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc": "2.0","id":1,"method":"getConfirmedBlocksWithLimit","params":[5, 3]}
'
```

Result:
```json
{"jsonrpc":"2.0","result":[5,6,7],"id":1}
```

### getConfirmedSignaturesForAddress

**DEPRECATED: Please use getConfirmedSignaturesForAddress2 instead**

Returns a list of all the confirmed signatures for transactions involving an address, within a specified Slot range. Max range allowed is 10,000 Slots

#### Parameters:

- `<string>` - account address as base-58 encoded string
- `<u64>` - start slot, inclusive
- `<u64>` - end slot, inclusive

#### Results:

The result field will be an array of:

- `<string>` - transaction signature as base-58 encoded string

The signatures will be ordered based on the Slot in which they were confirmed in, from lowest to highest Slot

#### Example:

Request:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getConfirmedSignaturesForAddress",
    "params": [
      "6H94zdiaYfRfPfKjYLjyr2VFBg6JHXygy84r3qhc3NsC",
      0,
      100
    ]
  }
'
```

Result:
```json
{
  "jsonrpc": "2.0",
  "result": [
    "35YGay1Lwjwgxe9zaH6APSHbt9gYQUCtBWTNL3aVwVGn9xTFw2fgds7qK5AL29mP63A9j3rh8KpN1TgSR62XCaby",
    "4bJdGN8Tt2kLWZ3Fa1dpwPSEkXWWTSszPSf1rRVsCwNjxbbUdwTeiWtmi8soA26YmwnKD4aAxNp8ci1Gjpdv4gsr",
    "4LQ14a7BYY27578Uj8LPCaVhSdJGLn9DJqnUJHpy95FMqdKf9acAhUhecPQNjNUy6VoNFUbvwYkPociFSf87cWbG"
  ],
  "id": 1
}
```

### getConfirmedSignaturesForAddress2

Returns confirmed signatures for transactions involving an address backwards in time from the provided signature or most recent confirmed block

#### Parameters:
* `<string>` - account address as base-58 encoded string
* `<object>` - (optional) Configuration object containing the following fields:
  * `limit: <number>` - (optional) maximum transaction signatures to return (between 1 and 1,000, default: 1,000).
  * `before: <string>` - (optional) start searching backwards from this transaction signature. If not provided the search starts from the top of the highest max confirmed block.
  * `until: <string>` - (optional) search until this transaction signature, if found before limit reached.

#### Results:
The result field will be an array of transaction signature information, ordered from newest to oldest transaction:
* `<object>`
  * `signature: <string>` - transaction signature as base-58 encoded string
  * `slot: <u64>` - O slot que contém o bloco com a transação
  * `err: <object | null>` - Erro se a transação falhou, null se a transação for bem-sucedida. [Definições TransactionError](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
  * `memo: <string |null>` - Memo associado à transação, nulo se nenhum memorando estiver presente

#### Exemplo:
Solicitação:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getConfirmedSignaturesForAddress2",
    "params": [
      "Vote111111111111111111111111111111111111111",
      {
        "limit": 1
      }
    ]
  }
'
```

Resultados:
```json
{
  "jsonrpc": "2.0",
  "result": [
    {
      "err": null,
      "memo": null,
      "signature": "5h6xBEauJ3PK6SWCZ1PGjBvj8vDdWG3KpwATGy1ARAXFSDwt8GFXM7W5Ncn16wmqokgpiKRLuS83KUxyZyv2sUYv",
      "slot": 114
    }
  ],
  "id": 1
}

```

### TransaçãoConfirmada

Retorna os detalhes da transação para uma transação confirmada

#### Parâmetros:

- `<string>` - assinatura de transação como string codificada base-58 N tentativas de codificação para usar analisadores de instruções específicos do programa para retornar dados mais legíveis e explícitos na transação `. Lista de essage.instructions`. Se "jsonParsed" for solicitado, mas um analisador não puder ser encontrado, a instrução voltará para a codificação regular de JSON (`contas`, `dados`, e `campos do programIdIndex`).
- `<string>` - (opcional) codificação para a transação retornada, ou "json", "jsonParsed", "base58" (*slow*), ou "base64". Se o parâmetro não for fornecido, a codificação padrão é JSON.

#### Resultados:

- `<null>` - se a transação não foi encontrada ou não foi confirmada
- `<object>` - Se a transação for confirmada, um objeto com os seguintes campos:
  - `slot: <u64>` - o slot em que a transação foi processada
  - `transação: <object|[string,encoding]>` - [objeto da transação](#transaction-structure) , seja em formato JSON ou dados binários codificados, dependendo do parâmetro de codificação
  - `meta: <object | null>` - objeto de metadados do status da transação:
    - `err: <object | null>` - Erro se a transação falhou, null se a transação for bem-sucedida. [Definições TransactionError](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
    - `taxa: <u64>` - taxa que esta transação foi cobrada, como u64 inteiro
    - `pré-saldos: <array>` - array de saldos de conta u64 de antes da transação ser processada
    - `pré-saldos: <array>` - array de saldos de conta u64 de antes da transação ser processada
    - `Instruções para o ner: <array|undefined>` - Lista de [instruções internas](#inner-instructions-structure) ou omitido se a gravação de instrução interna ainda não foi habilitada durante esta transação
    - `Mensagens de log: <array>` - matriz de mensagens de log de string ou omitido se a gravação de log ainda não foi habilitada durante esta transação
    - PRETERADO: `status: <object>` - Status da transação
      - `"Ok": <null>` - Transação bem sucedida
      - `"Err": <ERR>` - A transação falhou com TransactionError

#### Exemplo:
Solicitação:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getConfirmedTransaction",
    "params": [
      "2nBhEBYYvfaAe16UMNqRHre4YNSskvuYgx3M6E4JP1oDYvZEJHvoPzyUidNgNX5r9sTyN1J9UxtbCXy2rqYcuyuv",
      "json"
    ]
  }
'
```

Resultados:
```json
{
  "jsonrpc": "2.0",
  "result": {
    "meta": {
      "err": null,
      "fee": 5000,
      "innerInstructions": [],
      "postBalances": [
        499998932500,
        26858640,
        1,
        1,
        1
      ],
      "preBalances": [
        499998937500,
        26858640,
        1,
        1,
        1
      ],
      "status": {
        "Ok": null
      }
    },
    "slot": 430,
    "transaction": {
      "message": {
        "accountKeys": [
          "3UVYmECPPMZSCqWKfENfuoTv51fTDTWicX9xmBD2euKe",
          "AjozzgE83A3x1sHNUR64hfH7zaEBWeMaFuAN9kQgujrc",
          "SysvarS1otHashes111111111111111111111111111",
          "SysvarC1ock11111111111111111111111111111111",
          "Vote111111111111111111111111111111111111111"
        ],
        "header": {
          "numReadonlySignedAccounts": 0,
          "numReadonlyUnsignedAccounts": 3,
          "numRequiredSignatures": 1
        },
        "instructions": [
          {
            "accounts": [
              1,
              2,
              3,
              0
            ],
            "data": "37u9WtQpcm6ULa3WRQHmj49EPs4if7o9f1jSRVZpm2dvihR9C8jY4NqEwXUbLwx15HBSNcP1",
            "programIdIndex": 4
          }
        ],
        "recentBlockhash": "mfcyqEXB3DnHXki6KjjmZck6YjmZLvpAByy2fj4nh6B"
      },
      "signatures": [
        "2nBhEBYYvfaAe16UMNqRHre4YNSskvuYgx3M6E4JP1oDYvZEJHvoPzyUidNgNX5r9sTyN1J9UxtbCXy2rqYcuyuv"
      ]
    }
  },
  "id": 1
}
```

#### Exemplo:
Solicitação:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getConfirmedTransaction",
    "params": [
      "2nBhEBYYvfaAe16UMNqRHre4YNSskvuYgx3M6E4JP1oDYvZEJHvoPzyUidNgNX5r9sTyN1J9UxtbCXy2rqYcuyuv",
      "json"
    ]
  }
'
```

Resultado:
```json
{
  "jsonrpc": "2. ",
  "result": {
    "blockTime": nulo,
    "blockhash": "3Eq21vXNB5s86c62bVuUfTeaMif1N2kUqRBmGRJhyTA",
    "parentSlot": 429,
    "previousBlockhash": "mfcyqEXB3DnHXki6KjjmZck6YjmZLvpAByy2fj4nh6B",
    "recompensas": [],
    "transações": [
      {
        "meta": {
          "err": null,
          "taxa": 5000,
          "innerInstructions": [],
          "logMessages": [],
          "postBalances": [
            499998932500,
            26858640,
            1,
            1
            1
          ],
          "preSaldos": [
            499998937500,
            26858640,
            1,
            1,
            1
          ],
          "status": {
            "Ok": nulo
          }
        },
        "transacção": [
          "AVj7dxHlQ9IrvdYVIjuiRFs1jLaDMHixgrv+qtHBwz51L4/ImLZhszwiyEJDIp7xeBSpm/TX5B7mYzxa+fPOMw0BAAMFJVqLw+hJYheizSoYlLm53KzgT82cDVmazarqQKG2GQsLgiqktA+a+FDR4/7xnDX7rsusMwryYVUdiz1B1Q3KzgLm53KzgT82cDVmazarqQQKQKQQKG2GQsLgiqktLgiqkt+F7xA
          "base64"
        ]
      }
    ]
  },
  "id": 1
}
```

### getEpochInfo

Retorna informações sobre o período atual

#### Parâmetros:

- `<object>` - (opcional) [Compromisso](jsonrpc-api.md#configuring-state-commitment)

#### Resultados:

A saída da resposta será um objeto JSON com os seguintes campos:

- `absoluteSlot: <u64>`, o slot atual
- `absoluteSlot: <u64>`, o slot atual
- `epoch: <u64>`, the current epoch
- `slotIndex: <u64>`, the current slot relative to the start of the current epoch
- `slotsInEpoch: <u64>`, the number of slots in this epoch

#### Example:

Request:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getEpochInfo"}
'
```

Result:
```json
{
  "jsonrpc": "2.0",
  "result": {
    "absoluteSlot": 166598,
    "blockHeight": 166500,
    "epoch": 27,
    "slotIndex": 2790,
    "slotsInEpoch": 8192
  },
  "id": 1
}
```

### getEpochSchedule

Returns epoch schedule information from this cluster's genesis config

#### Parameters:

None

#### Results:

The result field will be an object with the following fields:

- `slotsPerEpoch: <u64>`, the maximum number of slots in each epoch
- `leaderScheduleSlotOffset: <u64>`, the number of slots before beginning of an epoch to calculate a leader schedule for that epoch
- `warmup: <bool>`, whether epochs start short and grow
- `firstNormalEpoch: <u64>`, first normal-length epoch, log2(slotsPerEpoch) - log2(MINIMUM_SLOTS_PER_EPOCH)
- `firstNormalSlot: <u64>`, MINIMUM_SLOTS_PER_EPOCH \* (2.pow(firstNormalEpoch) - 1)

#### Example:

Request:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getEpochSchedule"}
'
```

Result:
```json
{
  "jsonrpc": "2.0",
  "result": {
    "firstNormalEpoch": 8,
    "firstNormalSlot": 8160,
    "leaderScheduleSlotOffset": 8192,
    "slotsPerEpoch": 8192,
    "warmup": true
  },
  "id": 1
}
```

### getFeeCalculatorForBlockhash

Returns the fee calculator associated with the query blockhash, or `null` if the blockhash has expired

#### Parameters:

- `<string>` - query blockhash as a Base58 encoded string
- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

The result will be an RpcResponse JSON object with `value` equal to:

- `<null>` - if the query blockhash has expired
- `<object>` - otherwise, a JSON object containing:
  - `feeCalculator: <object>`, `FeeCalculator` object describing the cluster fee rate at the queried blockhash

#### Example:

Request:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getFeeCalculatorForBlockhash",
    "params": [
      "GJxqhuxcgfn5Tcj6y3f8X4FeCDd2RQ6SnEMo1AAxrPRZ"
    ]
  }
'
```

Result:
```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 221
    },
    "value": {
      "feeCalculator": {
        "lamportsPerSignature": 5000
      }
    }
  },
  "id": 1
}
```

### getFeeRateGovernor

Returns the fee rate governor information from the root bank

#### Parameters:

None

#### Results:

The `result` field will be an `object` with the following fields:

- `burnPercent: <u8>`, Percentage of fees collected to be destroyed
- `maxLamportsPerSignature: <u64>`, Largest value `lamportsPerSignature` can attain for the next slot
- `minLamportsPerSignature: <u64>`, Smallest value `lamportsPerSignature` can attain for the next slot
- `targetLamportsPerSignature: <u64>`, Desired fee rate for the cluster
- `targetSignaturesPerSlot: <u64>`, Desired signature rate for the cluster

#### Exemplo:

Solicitação:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getBlockCommitment","params":}
'
```

Resultados:
```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 54
    },
    "value": {
      "feeRateGovernor": {
        "burnPercent": 50,
        "maxLamportsPerSignature": 100000,
        "minLamportsPerSignature": 5000,
        "targetLamportsPerSignature": 10000,
        "targetSignaturesPerSlot": 20000
      }
    }
  },
  "id": 1
}
```

### Taxas

Retorna um hash de bloco recente do livro, uma programação de taxas que pode ser usada para calcular o custo de enviar uma transação usando ela e o último slot em que o blockhash será válido.

#### Parâmetros:

- `<object>` - (opcional) [Compromisso](jsonrpc-api.md#configuring-state-commitment)

#### Resultados:

O resultado será um objeto JSON de RpcResponse com `valor` definido para um objeto JSON com os seguintes campos:

- `blockhash: <string>` - uma Hash como string codificada base-58
- `feeCalculator: <object>` - FeeCalculator object, o cronograma de taxas para esse hash do bloco
- `lastValidSlot: <u64>` - último slot em que um blockhash será válido

#### Exemplo:

Solicitação:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getBlockCommitment","params":}
'
```

Resultados:
```json
{
  "jsonrpc": "2. ",
  "result": {
    "context": {
      "slot": 1
    },
    "value": {
      "blockhash": "CSymwgTNX1j3E4qhKfJAUE41nBWEwXufoYryPbkde5R",
      "feeCalculator": {
        "lamportsPerSignature": 5000
      },
      "lastValidSlot": 297
    }
  },
  "id": 1
}
```

### getFirstDisponívelBloco

Retorna o slot do menor bloco confirmado que não foi eliminado do livro razão

#### Parâmetros:

Nenhuma

#### Resultados:

- `<u64>` - Slot

#### Exemplo:

Solicitação:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getBlockCommitment","params":}
'
```

Resultados:
```json
{"jsonrpc":"2.0","resultado ":250000,"id":1}
```

### Hash

Retorna o hash da gênesis

#### Parâmetros:

Nenhuma

#### Resultados:

- `blockhash: <string>` - uma Hash como string codificada base-58

#### Exemplo:

Solicitação:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getBlockCommitment","params":}
'
```

Resultados:
```json
{"jsonrpc":"2.0","result":"GH7ome3EiwEr7tu9JuTh2dpYWBJ3z69Xm1ZE3MEE6JC","id":1}
```

### ObterSaúde

Retorna a saúde atual do nó.

Se um ou mais argumentos `--trusted-validator` são fornecidos para `solana-validator`, "ok" é retornado quando o nó tem dentro de `HEALTH_CHECK_SLOT_DISTANCE` dos slots do mais fiável validador caso contrário, um erro é devolvido.  "ok" é sempre retornado se nenhum validador confiável é fornecido.

#### Parâmetros:

Nenhuma

#### Resultados:

Se o nó estiver saudável: "ok" se o nó não estiver saudável, uma resposta de erro de RPC JSON é retornada.  Especialmente da resposta ao erro são **UNSTABLE** e podem mudar no futuro


#### Example:

Request:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getHealth"}
'
```

Healthy Result:
```json
{"jsonrpc":"2.0","result": "ok","id":1}
```

Unhealthy Result (generic):
```json
{
  "jsonrpc": "2.0",
  "error": {
    "code": -32005,
    "message": "Node is unhealthy",
    "data": {}
  },
  "id": 1
}
```

Unhealthy Result (if additional information is available)
```json
{
  "jsonrpc": "2.0",
  "error": {
    "code": -32005,
    "message": "Node is behind by 42 slots",
    "data": {
      "numSlotsBehind": 42
    }
  },
  "id": 1
}
```

### getIdentity

Returns the identity pubkey for the current node

#### Parameters:

None

#### Results:

The result field will be a JSON object with the following fields:

- `identity`, the identity pubkey of the current node \(as a base-58 encoded string\)

#### Example:

Request:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getIdentity"}
'
```

Result:
```json
{"jsonrpc":"2.0","result":{"identity": "2r1F4iWqVcb8M1DbAjQuFpebkQHY9hcVU4WuW2DJBppN"},"id":1}
```

### getInflationGovernor

Returns the current inflation governor

#### Parameters:

- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

The result field will be a JSON object with the following fields:

- `initial: <f64>`, the initial inflation percentage from time 0
- `terminal: <f64>`, terminal inflation percentage
- `taper: <f64>`, rate per year at which inflation is lowered
- `foundation: <f64>`, percentage of total inflation allocated to the foundation
- `foundationTerm: <f64>`, duration of foundation pool inflation in years

#### Example:

Request:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getInflationGovernor"}
'
```

Result:
```json
{
  "jsonrpc": "2.0",
  "result": {
    "foundation": 0.05,
    "foundationTerm": 7,
    "initial": 0.15,
    "taper": 0.15,
    "terminal": 0.015
  },
  "id": 1
}
```

### getInflationRate

Returns the specific inflation values for the current epoch

#### Parameters:

None

#### Results:

The result field will be a JSON object with the following fields:

- `total: <f64>`, total inflation
- `validator: <f64>`, inflation allocated to validators
- `foundation: <f64>`, inflation allocated to the foundation
- `epoch: <f64>`, epoch for which these values are valid

#### Example:

Request:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getInflationRate"}
'
```

Resultados:
```json
{"jsonrpc":"2.0","result":{"epoch":100,"foundation":0.001,"total":0.149,"validator":0.148},"id":1}
```

### getLargestAccounts

Retorna as 20 maiores contas, por saldo dos lâmpados

#### Parâmetros:

- `<object>` - (opcional) Objeto de configuração contendo os seguintes campos opcionais:
  - `<object>` - (opcional) [Compromisso](jsonrpc-api.md#configuring-state-commitment)
  - (opcional) `filtro: <string>` - resultados de filtro por tipo de conta; atualmente suportados: `circulando nonCirculating`

#### Resultados:

O resultado será um objeto JSON RpcResponse com `valor` igual a um array de:

- `<object>` - caso contrário, um objeto JSON contendo:
  - `endereço: <string>`, endereço codificado base-58 da conta
  - `lâmpadas: <u64>`, número de lâmpadas na conta, como um u64

#### Exemplo:

Solicitação:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getBlockCommitment","params":}
'
```

Resultado:
```json
{
  "jsonrpc": "2. ",
  "result": {
    "context": {
      "slot": 54
    },
    "valor": [
      {
        "lamports": 999974,
        "address": "99P8ZgtJYe1buSK8JXkvpLh8xPsCFuLYhz9hQFNw93WJ"
      },
      {
        "lamports": 42,
        "endereço": "uPwWLo16MVehpyWqsLkK3Ka8nLowWvAHbBChqv2FZeL"
      },
      {
        "lamports": 42,
        "endereço": "aYJCgU7REfu3XF8b3QhkqgqQvLizx8zxuLBHA25PzDS"
      },
      {
        "lamports": 42,
        "address": "CTvHVtQ4gd4gUcw3bdVgZJqApXE9nCbbbP4VTS5wE1D"
      },
      {
        "lamports": 20,
        "address": "4fq3xJ6kfrh9RkJQsmVd5gNMvJbuSHfErywvEjNQDPxu"
      },
      {
        "lamports": 4,
        "endereço": "AXJADheGVp9cruP8WYu46oNkRbeASngN5fPCMVGQqNHa"
      },
      {
        "lamports": 2,
        "address": "8NT8yS6LiwNprgW4yM1jPPow7CwRUotddBVkrkWgYp24"
      },
      {
        "lamports": 1,
        "address": "SysvarEpochAgen1e111111111111111111111111"
      },
      {
        "lamports": 1,
        "endereço": "11111111111111111111111111111111"
      },
      {
        "lamports": 1,
        "endereço": "Stake11111111111111111111111111111111111111111111"
      },
      {
        "lamports": 1,
        "endereço": "SysvarC1ock111111111111111111111111111111111111"
      },
      {
        "lamports": 1,
        "address": "StakeConfig11111111111111111111111111111111111111"
      },
      {
        "lamports": 1,
        "address": "SysvarRent1111111111111111111111111111111111111"
      },
      {
        "lamports": 1,
        "address": "Config11111111111111111111111111111111111111111"
      },
      {
        "lamports": 1,
        "address": "SysvarStakeHistory11111111111111111111111"
      },
      {
        "lamports": 1,
        "endereço": "SysvarRecentB1ockHashes111111111111111111"
      },
      {
        "lamports": 1,
        "address": "SysvarFees1111111111111111111111111111111111111"
      },
      {
        "lamports": 1,
        "endereço": "Vote11111111111111111111111111111111111111111111111"
      }
    ]
  },
  "id": 1
}
```

### getLeaderSchedule

Retorna o cronograma de líder de uma época

#### Parâmetros:

- `<u64>` - (opcional) Buscar a agenda de líderes para o período que corresponde ao espaço fornecido. Se não for especificado, o cronograma de líder para o período atual é buscado
- `<object>` - (opcional) [Compromisso](jsonrpc-api.md#configuring-state-commitment)

#### Resultados:

- `<null>` - se o tempo solicitado não for encontrado
- `<object>` - caso contrário, o campo resultado será um dicionário das chaves públicas dos líderes \(como cadeias codificadas de base-58\) e seus índices de espaços de líder correspondentes como valores (índices são relativos ao primeiro espaço no epoche solicitado)

#### Exemplo:

Solicitação:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getBlockCommitment","params":}
'
```

Resultado:
```json
{
  "jsonrpc":"2.0",
  "result":{
    "4Qkev8aNZcqFNSRhQzwyLMFSsi94jHqE8WNVTJzTP99F":[0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31,32,33,34,35,36,37,38,39,40,41,42,43,44,45,46,47,48,49,50,51,52,53,54,55,56,57,58,59,60,61,62,63]
  },
  "id":1
}
```

### getMinimumBalanceForRentExemption

Retorna o saldo mínimo necessário para isentar o aluguel de conta.

#### Parâmetros:

- `<usize>` - duração dos dados da conta
- `<object>` - (opcional) [Compromisso](jsonrpc-api.md#configuring-state-commitment)

#### Resultados:

- `<u64>` - lâmpadas mínimas necessárias na conta

#### Exemplo:

Solicitação:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getBlockCommitment","params":[50]}
'
```

Resultado:
```json
{"jsonrpc":"2.0","resultado ":500,"id":1}
```

### ContasMultiplas

Retorna as informações de conta de uma lista de chaves de publicação

#### Parâmetros:

- `<array>` - Publique a chave da conta a ser consultada, como string codificada com base-58
- `<object>` - (opcional) Objeto de configuração contendo os seguintes campos opcionais:
  - `<object>` - (opcional) [Compromisso](jsonrpc-api.md#configuring-state-commitment)
  - `encoding: <string>` - encoding for Account data, either "base58" (*slow*), "base64", "base64+zstd", or "jsonParsed". "base58" is limited to Account data of less than 128 bytes. "base64" will return base64 encoded data for Account data of any size. "base64+zstd" compresses the Account data using [Zstandard](https://facebook.github.io/zstd/) and base64-encodes the result. "jsonParsed" encoding attempts to use program-specific state parsers to return more human-readable and explicit account state data. If "jsonParsed" is requested but a parser cannot be found, the field falls back to "base64" encoding, detectable when the `data` field is type `<string>`.
  - (optional) `dataSlice: <object>` - limit the returned account data using the provided `offset: <usize>` and `length: <usize>` fields; only available for "base58", "base64" or "base64+zstd" encodings.


#### Results:

The result will be an RpcResponse JSON object with `value` equal to:

An array of:

- `<null>` - if the account at that Pubkey doesn't exist
- `<object>` - otherwise, a JSON object containing:
  - `lamports: <u64>`, number of lamports assigned to this account, as a u64
  - `owner: <string>`, base-58 encoded Pubkey of the program this account has been assigned to
  - `data: <[string, encoding]|object>`, data associated with the account, either as encoded binary data or JSON format `{<program>: <state>}`, depending on encoding parameter
  - `executable: <bool>`, boolean indicating if the account contains a program \(and is strictly read-only\)
  - `rentEpoch: <u64>`, the epoch at which this account will next owe rent, as u64

#### Example:

Request:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getMultipleAccounts",
    "params": [
      [
        "vines1vzrYbzLMRdu58ou5XTby4qAqVRLmqo36NKPTg",
        "4fYNw3dojWmQ4dXtSGE9epjRGy9pFSx62YypT7avPYvA"
      ],
      {
        "dataSlice": {
          "offset": 0,
          "length": 0
        }
      }
    ]
  }
'
```

Result:
```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1
    },
    "value": [
      {
        "data": [
          "AAAAAAEAAAACtzNsyJrW0g==",
          "base64"
        ],
        "executable": false,
        "lamports": 1000000000,
        "owner": "11111111111111111111111111111111",
        "rentEpoch": 2
      },
      {
        "data": [
          "",
          "base64"
        ],
        "executable": false,
        "lamports": 5000000000,
        "owner": "11111111111111111111111111111111",
        "rentEpoch": 2
      }
    ]
  },
  "id": 1
}
```

#### Example:
Request:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getMultipleAccounts",
    "params": [
      [
        "vines1vzrYbzLMRdu58ou5XTby4qAqVRLmqo36NKPTg",
        "4fYNw3dojWmQ4dXtSGE9epjRGy9pFSx62YypT7avPYvA"
      ],
      {
        "encoding": "base58"
      }
    ]
  }
'
```

Result:
```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1
    },
    "value": [
      {
        "data": [
          "11116bv5nS2h3y12kD1yUKeMZvGcKLSjQgX6BeV7u1FrjeJcKfsHRTPuR3oZ1EioKtYGiYxpxMG5vpbZLsbcBYBEmZZcMKaSoGx9JZeAuWf",
          "base58"
        ],
        "executable": false,
        "lamports": 1000000000,
        "owner": "11111111111111111111111111111111",
        "rentEpoch": 2
      },
      {
        "data": [
          "",
          "base58"
        ],
        "executable": false,
        "lamports": 5000000000,
        "owner": "11111111111111111111111111111111",
        "rentEpoch": 2
      }
    ]
  },
  "id": 1
}
```

### getProgramAccounts

Returns all accounts owned by the provided program Pubkey

#### Parameters:

- `<string>` - Pubkey of program, as base-58 encoded string
- `<object>` - (optional) Configuration object containing the following optional fields:
  - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)
  - `encoding: <string>` - encoding for Account data, either "base58" (*slow*), "base64", "base64+zstd", or "jsonParsed". "base58" is limited to Account data of less than 128 bytes. "base64" will return base64 encoded data for Account data of any size. "base64+zstd" compresses the Account data using [Zstandard](https://facebook.github.io/zstd/) and base64-encodes the result. "jsonParsed" encoding attempts to use program-specific state parsers to return more human-readable and explicit account state data. If "jsonParsed" is requested but a parser cannot be found, the field falls back to "base64" encoding, detectable when the `data` field is type `<string>`.
  - (optional) `dataSlice: <object>` - limit the returned account data using the provided `offset: <usize>` and `length: <usize>` fields; only available for "base58", "base64" or "base64+zstd" encodings.
  - (optional) `filters: <array>` - filter results using various [filter objects](jsonrpc-api.md#filters); account must meet all filter criteria to be included in results

##### Filters:
- `memcmp: <object>` - compares a provided series of bytes with program account data at a particular offset. Fields:
  - `offset: <usize>` - offset into program account data to start comparison
  - `bytes: <string>` - data to match, as base-58 encoded string

- `dataSize: <u64>` - compares the program account data length with the provided data size

#### Results:

The result field will be an array of JSON objects, which will contain:

- `pubkey: <string>` - the account Pubkey as base-58 encoded string
- `conta: <object>` - um objeto JSON, com os seguintes subcampos:
   - `lamports: <u64>`, number of lamports assigned to this account, as a u64
   - `owner: <string>`, base-58 encoded Pubkey of the program this account has been assigned to `data: <[string,encoding]|object>`, data associated with the account, either as encoded binary data or JSON format `{<program>: <state>}`, depending on encoding parameter
   - `executable: <bool>`, boolean indicating if the account contains a program \(and is strictly read-only\)
   - `rentEpoch: <u64>`, the epoch at which this account will next owe rent, as u64

#### Example:
Request:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getProgramAccounts", "params":["4Nd1mBQtrMJVYVfKf2PJy9NZUZdTAsp7D4xWLs4gDB4T"]}
'
```

Result:
```json
{
  "jsonrpc": "2.0",
  "result": [
    {
      "account": {
        "data": "2R9jLfiAQ9bgdcw6h8s44439",
        "executable": false,
        "lamports": 15298080,
        "owner": "4Nd1mBQtrMJVYVfKf2PJy9NZUZdTAsp7D4xWLs4gDB4T",
        "rentEpoch": 28
      },
      "pubkey": "CxELquR1gPP8wHe33gZ4QxqGB3sZ9RSwsJ2KshVewkFY"
    }
  ],
  "id": 1
}
```

#### Example:
Request:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getProgramAccounts",
    "params": [
      "4Nd1mBQtrMJVYVfKf2PJy9NZUZdTAsp7D4xWLs4gDB4T",
      {
        "filters": [
          {
            "dataSize": 17
          },
          {
            "memcmp": {
              "offset": 4,
              "bytes": "3Mc6vR"
            }
          }
        ]
      }
    ]
  }
'
```

Result:
```json
{
  "jsonrpc": "2.0",
  "result": [
    {
      "account": {
        "data": "2R9jLfiAQ9bgdcw6h8s44439",
        "executable": false,
        "lamports": 15298080,
        "owner": "4Nd1mBQtrMJVYVfKf2PJy9NZUZdTAsp7D4xWLs4gDB4T",
        "rentEpoch": 28
      },
      "pubkey": "CxELquR1gPP8wHe33gZ4QxqGB3sZ9RSwsJ2KshVewkFY"
    }
  ],
  "id": 1
}
```

### getRecentBlockhash

Returns a recent block hash from the ledger, and a fee schedule that can be used to compute the cost of submitting a transaction using it.

#### Parameters:

- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

An RpcResponse containing a JSON object consisting of a string blockhash and FeeCalculator JSON object.

- `RpcResponse<object>` - RpcResponse JSON object with `value` field set to a JSON object including:
- `blockhash: <string>` - a Hash as base-58 encoded string
- `feeCalculator: <object>` - FeeCalculator object, the fee schedule for this block hash

#### Example:

Request:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d 'i
  {"jsonrpc":"2.0","id":1, "method":"getRecentBlockhash"}
'
```

Result:
```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1
    },
    "value": {
      "blockhash": "CSymwgTNX1j3E4qhKfJAUE41nBWEwXufoYryPbkde5RR",
      "feeCalculator": {
        "lamportsPerSignature": 5000
      }
    }
  },
  "id": 1
}
```

### getRecentPerformanceSamples

Returns a list of recent performance samples, in reverse slot order. Performance samples are taken every 60 seconds and include the number of transactions and slots that occur in a given time window.

#### Parameters:
- `limit: <usize>` - (optional) number of samples to return (maximum 720)

#### Results:

An array of:

- `RpcPerfSample<object>`
  - `slot: <u64>` - Slot in which sample was taken at
  - `numTransactions: <u64>` - Number of transactions in sample
  - `numSlots: <u64>` - Number of slots in sample
  - `samplePeriodSecs: <u16>` - Number of seconds in a sample window

#### Example:

Request:
```bash
// Request
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getRecentPerformanceSamples", "params": [4]}
'
```

Result:
```json
{
  "jsonrpc": "2.0",
  "result": [
    {
      "numSlots": 126,
      "numTransactions": 126,
      "samplePeriodSecs": 60,
      "slot": 348125
    },
    {
      "numSlots": 126,
      "numTransactions": 126,
      "samplePeriodSecs": 60,
      "slot": 347999
    },
    {
      "numSlots": 125,
      "numTransactions": 125,
      "samplePeriodSecs": 60,
      "slot": 347873
    },
    {
      "numSlots": 125,
      "numTransactions": 125,
      "samplePeriodSecs": 60,
      "slot": 347748
    }
  ],
  "id": 1
}
```


### getSnapshotSlot

Returns the highest slot that the node has a snapshot for

#### Parameters:

None

#### Results:

- `<u64>` - Snapshot slot

#### Example:

Request:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getSnapshotSlot"}
'
```

Result:
```json
{"jsonrpc":"2.0","result":100,"id":1}
```

Result when the node has no snapshot:
```json
{"jsonrpc":"2.0","error":{"code":-32008,"message":"No snapshot"},"id":1}
```

### getSignatureStatuses

Returns the statuses of a list of signatures. Unless the `searchTransactionHistory` configuration parameter is included, this method only searches the recent status cache of signatures, which retains statuses for all active slots plus `MAX_RECENT_BLOCKHASHES` rooted slots.

#### Parameters:

- `<array>` - An array of transaction signatures to confirm, as base-58 encoded strings
- `<object>` - (optional) Configuration object containing the following field:
  - `searchTransactionHistory: <bool>` - if true, a Solana node will search its ledger cache for any signatures not found in the recent status cache

#### Results:

An RpcResponse containing a JSON object consisting of an array of TransactionStatus objects.

- `RpcResponse<object>` - RpcResponse JSON object with `value` field:

An array of:

- `<null>` - Unknown transaction
- `<object>`
  - `slot: <u64>` - The slot the transaction was processed
  - `confirmations: <usize | null>` - Number of blocks since signature confirmation, null if rooted, as well as finalized by a supermajority of the cluster
  - `err: <object | null>` - Error if transaction failed, null if transaction succeeded. [TransactionError definitions](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
  - `confirmationStatus: <string | null>` - The transaction's cluster confirmation status; either `processed`, `confirmed`, or `finalized`. See [Commitment](jsonrpc-api.md#configuring-state-commitment) for more on optimistic confirmation.
  - DEPRECATED: `status: <object>` - Transaction status
    - `"Ok": <null>` - Transaction was successful
    - `"Err": <ERR>` - Transaction failed with TransactionError

#### Example:

Request:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getSignatureStatuses",
    "params": [
      [
        "5VERv8NMvzbJMEkV8xnrLkEaWRtSz9CosKDYjCJjBRnbJLgp8uirBgmQpjKhoR4tjF3ZpRzrFmBV6UjKdiSZkQUW",
        "5j7s6NiJS3JAkvgkoc18WVAsiSaci2pxB2A6ueCJP4tprA2TFg9wSyTLeYouxPBJEMzJinENTkpA52YStRW5Dia7"
      ]
    ]
  }
'
```

Result:
```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 82
    },
    "value": [
      {
        "slot": 72,
        "confirmations": 10,
        "err": null,
        "status": {
          "Ok": null
        },
        "confirmationStatus": "confirmed",
      },
      null
    ]
  },
  "id": 1
}
```

#### Example:
Request:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getSignatureStatuses",
    "params": [
      [
        "5VERv8NMvzbJMEkV8xnrLkEaWRtSz9CosKDYjCJjBRnbJLgp8uirBgmQpjKhoR4tjF3ZpRzrFmBV6UjKdiSZkQUW"
      ],
      {
        "searchTransactionHistory": true
      }
    ]
  }
'
```

Result:
```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 82
    },
    "value": [
      {
        "slot": 48,
        "confirmations": null,
        "err": null,
        "status": {
          "Ok": null
        },
        "confirmationStatus": "finalized",
      },
      null
    ]
  },
  "id": 1
}
```

### getSlot

Returns the current slot the node is processing

#### Parameters:

- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

- `<u64>` - Current slot

#### Example:

Request:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getSlot"}
'
```

Result:
```json
{"jsonrpc":"2.0","result":1234,"id":1}
```

### getSlotLeader

Returns the current slot leader

#### Parameters:

- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

- `<string>` - Node identity Pubkey as base-58 encoded string

#### Example:

Request:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getSlotLeader"}
'
```

Result:
```json
{"jsonrpc":"2.0","result":"ENvAW7JScgYq6o4zKZwewtkzzJgDzuJAFxYasvmEQdpS","id":1}
```

### getStakeActivation

Returns epoch activation information for a stake account

#### Parameters:

* `<string>` - Pubkey of stake account to query, as base-58 encoded string
* `<object>` - (optional) Configuration object containing the following optional fields:
  * (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)
  * (optional) `epoch: <u64>` - epoch for which to calculate activation details. If parameter not provided, defaults to current epoch.

#### Results:

The result will be a JSON object with the following fields:

* `state: <string` - the stake account's activation state, one of: `active`, `inactive`, `activating`, `deactivating`
* `active: <u64>` - stake active during the epoch
* `inactive: <u64>` - stake inactive during the epoch

#### Example:
Request:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getStakeActivation", "params": ["CYRJWqiSjLitBAcRxPvWpgX3s5TvmN2SuRY3eEYypFvT"]}
'
```

Result:
```json
{"jsonrpc":"2.0","result":{"active":197717120,"inactive":0,"state":"active"},"id":1}
```

#### Example:
Request:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getStakeActivation",
    "params": [
      "CYRJWqiSjLitBAcRxPvWpgX3s5TvmN2SuRY3eEYypFvT",
      {
        "epoch": 4
      }
    ]
  }
'
```

Result:
```json
{
  "jsonrpc": "2.0",
  "result": {
    "active": 124429280,
    "inactive": 73287840,
    "state": "activating"
  },
  "id": 1
}
```

### getSupply

Returns information about the current supply.

#### Parameters:

- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

The result will be an RpcResponse JSON object with `value` equal to a JSON object containing:

- `total: <u64>` - Total supply in lamports
- `circulating: <u64>` - Circulating supply in lamports
- `nonCirculating: <u64>` - Non-circulating supply in lamports
- `nonCirculatingAccounts: <array>` - an array of account addresses of non-circulating accounts, as strings

#### Example:

Request:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getSupply"}
'
```

Result:
```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1114
    },
    "value": {
      "circulating": 16000,
      "nonCirculating": 1000000,
      "nonCirculatingAccounts": [
        "FEy8pTbP5fEoqMV1GdTz83byuA8EKByqYat1PKDgVAq5",
        "9huDUZfxoJ7wGMTffUE7vh1xePqef7gyrLJu9NApncqA",
        "3mi1GmwEE3zo2jmfDuzvjSX9ovRXsDUKHvsntpkhuLJ9",
        "BYxEJTDerkaRWBem3XgnVcdhppktBXa2HbkHPKj2Ui4Z"
      ],
      "total": 1016000
    }
  },
  "id": 1
}
```

### getTokenAccountBalance

Returns the token balance of an SPL Token account. **UNSTABLE**

#### Parameters:

- `<string>` - Pubkey of Token account to query, as base-58 encoded string
- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

The result will be an RpcResponse JSON object with `value` equal to a JSON object containing:

- `uiAmount: <f64>` - the balance, using mint-prescribed decimals
- `amount: <string>` - the raw balance without decimals, a string representation of u64
- `decimals: <u8>` - number of base 10 digits to the right of the decimal place

#### Example:

Request:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getTokenAccountBalance", "params": ["7fUAJdStEuGbc3sM84cKRL6yYaaSstyLSU4ve5oovLS7"]}
'
```

Result:
```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1114
    },
    "value": {
      "uiAmount": 98.64,
      "amount": "9864",
      "decimals": 2
    },
    "id": 1
  }
}
```

### getTokenAccountsByDelegate

Returns all SPL Token accounts by approved Delegate. **UNSTABLE**

#### Parameters:

- `<string>` - Pubkey of account delegate to query, as base-58 encoded string
- `<object>` - Either:
  * `mint: <string>` - Pubkey of the specific token Mint to limit accounts to, as base-58 encoded string; or
  * `programId: <string>` - Pubkey of the Token program ID that owns the accounts, as base-58 encoded string
- `<object>` - (optional) Configuration object containing the following optional fields:
  - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)
  - `encoding: <string>` - encoding for Account data, either "base58" (*slow*), "base64", "base64+zstd" or "jsonParsed". "jsonParsed" encoding attempts to use program-specific state parsers to return more human-readable and explicit account state data. If "jsonParsed" is requested but a valid mint cannot be found for a particular account, that account will be filtered out from results.
  - (optional) `dataSlice: <object>` - limit the returned account data using the provided `offset: <usize>` and `length: <usize>` fields; only available for "base58", "base64" or "base64+zstd" encodings.

#### Results:

The result will be an RpcResponse JSON object with `value` equal to an array of JSON objects, which will contain:

- `pubkey: <string>` - the account Pubkey as base-58 encoded string
- `account: <object>` - a JSON object, with the following sub fields:
   - `lamports: <u64>`, number of lamports assigned to this account, as a u64
   - `owner: <string>`, base-58 encoded Pubkey of the program this account has been assigned to
   - `data: <object>`, Token state data associated with the account, either as encoded binary data or in JSON format `{<program>: <state>}`
   - `executable: <bool>`, boolean indicating if the account contains a program \(and is strictly read-only\)
   - `rentEpoch: <u64>`, the epoch at which this account will next owe rent, as u64

#### Example:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getTokenAccountsByDelegate",
    "params": [
      "4Nd1mBQtrMJVYVfKf2PJy9NZUZdTAsp7D4xWLs4gDB4T",
      {
        "programId": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
      },
      {
        "encoding": "jsonParsed"
      }
    ]
  }
'
```

Result:
```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1114
    },
    "value": [
      {
        "data": {
          "program": "spl-token",
          "parsed": {
            "accountType": "account",
            "info": {
              "tokenAmount": {
                "amount": "1",
                "uiAmount": 0.1,
                "decimals": 1
              },
              "delegate": "4Nd1mBQtrMJVYVfKf2PJy9NZUZdTAsp7D4xWLs4gDB4T",
              "delegatedAmount": 1,
              "isInitialized": true,
              "isNative": false,
              "mint": "3wyAj7Rt1TWVPZVteFJPLa26JmLvdb1CAKEFZm3NY75E",
              "owner": "CnPoSPKXu7wJqxe59Fs72tkBeALovhsCxYeFwPCQH9TD"
            }
          }
        },
        "executable": false,
        "lamports": 1726080,
        "owner": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
        "rentEpoch": 4
      }
    ]
  },
  "id": 1
}
```

### getTokenAccountsByOwner

Returns all SPL Token accounts by token owner. **UNSTABLE**

#### Parameters:

- `<string>` - Pubkey of account owner to query, as base-58 encoded string
- `<object>` - Either:
  * `mint: <string>` - Pubkey of the specific token Mint to limit accounts to, as base-58 encoded string; or
  * `programId: <string>` - Pubkey of the Token program ID that owns the accounts, as base-58 encoded string
- `<object>` - (optional) Configuration object containing the following optional fields:
  - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)
  - `encoding: <string>` - encoding for Account data, either "base58" (*slow*), "base64", "base64+zstd" or "jsonParsed". "jsonParsed" encoding attempts to use program-specific state parsers to return more human-readable and explicit account state data. If "jsonParsed" is requested but a valid mint cannot be found for a particular account, that account will be filtered out from results.
  - (optional) `dataSlice: <object>` - limit the returned account data using the provided `offset: <usize>` and `length: <usize>` fields; only available for "base58", "base64" or "base64+zstd" encodings.

#### Results:

The result will be an RpcResponse JSON object with `value` equal to an array of JSON objects, which will contain:

- `pubkey: <string>` - the account Pubkey as base-58 encoded string
- `account: <object>` - a JSON object, with the following sub fields:
   - `lamports: <u64>`, number of lamports assigned to this account, as a u64
   - `owner: <string>`, base-58 encoded Pubkey of the program this account has been assigned to
   - `data: <object>`, Token state data associated with the account, either as encoded binary data or in JSON format `{<program>: <state>}`
   - `executable: <bool>`, boolean indicating if the account contains a program \(and is strictly read-only\)
   - `rentEpoch: <u64>`, the epoch at which this account will next owe rent, as u64

#### Example:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getTokenAccountsByOwner",
    "params": [
      "4Qkev8aNZcqFNSRhQzwyLMFSsi94jHqE8WNVTJzTP99F",
      {
        "mint": "3wyAj7Rt1TWVPZVteFJPLa26JmLvdb1CAKEFZm3NY75E"
      },
      {
        "encoding": "jsonParsed"
      }
    ]
  }
'
```

Result:
```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1114
    },
    "value": [
      {
        "data": {
          "program": "spl-token",
          "parsed": {
            "accountType": "account",
            "info": {
              "tokenAmount": {
                "amount": "1",
                "uiAmount": 0.1,
                "decimals": 1
              },
              "delegate": null,
              "delegatedAmount": 1,
              "isInitialized": true,
              "isNative": false,
              "mint": "3wyAj7Rt1TWVPZVteFJPLa26JmLvdb1CAKEFZm3NY75E",
              "owner": "4Qkev8aNZcqFNSRhQzwyLMFSsi94jHqE8WNVTJzTP99F"
            }
          }
        },
        "executable": false,
        "lamports": 1726080,
        "owner": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
        "rentEpoch": 4
      }
    ]
  },
  "id": 1
}
```

### getTokenLargestAccounts

Returns the 20 largest accounts of a particular SPL Token type. **UNSTABLE**

#### Parameters:

- `<string>` - Pubkey of token Mint to query, as base-58 encoded string
- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

The result will be an RpcResponse JSON object with `value` equal to an array of JSON objects containing:

- `address: <string>` - the address of the token account
- `uiAmount: <f64>` - the token account balance, using mint-prescribed decimals
- `amount: <string>` - the raw token account balance without decimals, a string representation of u64
- `decimals: <u8>` - number of base 10 digits to the right of the decimal place

#### Example:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getTokenLargestAccounts", "params": ["3wyAj7Rt1TWVPZVteFJPLa26JmLvdb1CAKEFZm3NY75E"]}
'
```

Result:
```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1114
    },
    "value": [
      {
        "address": "FYjHNoFtSQ5uijKrZFyYAxvEr87hsKXkXcxkcmkBAf4r",
        "amount": "771",
        "decimals": 2,
        "uiAmount": 7.71
      },
      {
        "address": "BnsywxTcaYeNUtzrPxQUvzAWxfzZe3ZLUJ4wMMuLESnu",
        "amount": "229",
        "decimals": 2,
        "uiAmount": 2.29
      }
    ]
  },
  "id": 1
}
```

### getTokenSupply

Returns the total supply of an SPL Token type. **UNSTABLE**

#### Parameters:

- `<string>` - Pubkey of token Mint to query, as base-58 encoded string
- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

The result will be an RpcResponse JSON object with `value` equal to a JSON object containing:

- `uiAmount: <f64>` - the total token supply, using mint-prescribed decimals
- `amount: <string>` - the raw total token supply without decimals, a string representation of u64
- `decimals: <u8>` - number of base 10 digits to the right of the decimal place

#### Example:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getTokenSupply", "params": ["3wyAj7Rt1TWVPZVteFJPLa26JmLvdb1CAKEFZm3NY75E"]}
'
```

Result:
```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1114
    },
    "value": {
      "uiAmount": 1000,
      "amount": "100000",
      "decimals": 2
    }
  },
  "id": 1
}
```

### getTransactionCount

Returns the current Transaction count from the ledger

#### Parameters:

- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

- `<u64>` - count

#### Example:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getTransactionCount"}
'

```

Result:
```json
{"jsonrpc":"2.0","result":268,"id":1}
```

### getVersion

Returns the current solana versions running on the node

#### Parameters:

None

#### Results:

The result field will be a JSON object with the following fields:

- `solana-core`, software version of solana-core
- `feature-set`, unique identifier of the current software's feature set

#### Example:

Request:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getVersion"}
'
```

Result:
```json
{"jsonrpc":"2.0","result":{"solana-core": "1.6.0"},"id":1}
```

### getVoteAccounts

Returns the account info and associated stake for all the voting accounts in the current bank.

#### Parameters:

- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

The result field will be a JSON object of `current` and `delinquent` accounts, each containing an array of JSON objects with the following sub fields:

- `votePubkey: <string>` - Vote account public key, as base-58 encoded string
- `nodePubkey: <string>` - Node public key, as base-58 encoded string
- `activatedStake: <u64>` - the stake, in lamports, delegated to this vote account and active in this epoch
- `epochVoteAccount: <bool>` - bool, whether the vote account is staked for this epoch
- `commission: <number>`, percentage (0-100) of rewards payout owed to the vote account
- `lastVote: <u64>` - Most recent slot voted on by this vote account
- `epochCredits: <array>` - History of how many credits earned by the end of each epoch, as an array of arrays containing: `[epoch, credits, previousCredits]`

#### Example:
Request:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getVoteAccounts"}
'
```

Result:
```json
{
  "jsonrpc": "2.0",
  "result": {
    "current": [
      {
        "commission": 0,
        "epochVoteAccount": true,
        "epochCredits": [
          [ 1, 64, 0 ],
          [ 2, 192, 64 ]
        ],
        "nodePubkey": "B97CCUW3AEZFGy6uUg6zUdnNYvnVq5VG8PUtb2HayTDD",
        "lastVote": 147,
        "activatedStake": 42,
        "votePubkey": "3ZT31jkAGhUaw8jsy4bTknwBMP8i4Eueh52By4zXcsVw"
      }
    ],
    "delinquent": [
      {
        "commission": 127,
        "epochVoteAccount": false,
        "epochCredits": [],
        "nodePubkey": "6ZPxeQaDo4bkZLRsdNrCzchNQr5LN9QMc9sipXv9Kw8f",
        "lastVote": 0,
        "activatedStake": 0,
        "votePubkey": "CmgCk4aMS7KW1SHX3s9K5tBJ6Yng2LBaC8MFov4wx9sm"
      }
    ]
  },
  "id": 1
}
```

### minimumLedgerSlot

Returns the lowest slot that the node has information about in its ledger. This value may increase over time if the node is configured to purge older ledger data

#### Parameters:

None

#### Results:

- `u64` - Minimum ledger slot

#### Example:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"minimumLedgerSlot"}
'

```

Result:
```json
{"jsonrpc":"2.0","result":1234,"id":1}
```

### requestAirdrop

Requests an airdrop of lamports to a Pubkey

#### Parameters:

- `<string>` - Pubkey of account to receive lamports, as base-58 encoded string
- `<integer>` - lamports, as a u64
- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment) (used for retrieving blockhash and verifying airdrop success)

#### Results:

- `<string>` - Transaction Signature of airdrop, as base-58 encoded string

#### Example:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"requestAirdrop", "params":["83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri", 50]}
'

```

Result:
```json
{"jsonrpc":"2.0","result":"5VERv8NMvzbJMEkV8xnrLkEaWRtSz9CosKDYjCJjBRnbJLgp8uirBgmQpjKhoR4tjF3ZpRzrFmBV6UjKdiSZkQUW","id":1}
```

### sendTransaction

Submits a signed transaction to the cluster for processing.

This method does not alter the transaction in any way; it relays the transaction created by clients to the node as-is.

If the node's rpc service receives the transaction, this method immediately succeeds, without waiting for any confirmations. A successful response from this method does not guarantee the transaction is processed or confirmed by the cluster.

While the rpc service will reasonably retry to submit it, the transaction could be rejected if transaction's `recent_blockhash` expires before it lands.

Use [`getSignatureStatuses`](jsonrpc-api.md#getsignaturestatuses) to ensure a transaction is processed and confirmed.

Before submitting, the following preflight checks are performed:

1. The transaction signatures are verified
2. The transaction is simulated against the bank slot specified by the preflight commitment. On failure an error will be returned. Preflight checks may be disabled if desired. It is recommended to specify the same commitment and preflight commitment to avoid confusing behavior.

The returned signature is the first signature in the transaction, which is used to identify the transaction ([transaction id](../../terminology.md#transanction-id)). This identifier can be easily extracted from the transaction data before submission.

#### Parameters:

- `<string>` - fully-signed Transaction, as encoded string
- `<object>` - (optional) Configuration object containing the following field:
  - `skipPreflight: <bool>` - if true, skip the preflight transaction checks (default: false)
  - `preflightCommitment: <string>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment) level to use for preflight (default: `"max"`).
  - `encoding: <string>` - (optional) Encoding used for the transaction data. Either `"base58"` (*slow*, **DEPRECATED**), or `"base64"`. (default: `"base58"`).

#### Results:

- `<string>` - First Transaction Signature embedded in the transaction, as base-58 encoded string ([transaction id](../../terminology.md#transanction-id))

#### Example:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "sendTransaction",
    "params": [
      "4hXTCkRzt9WyecNzV1XPgCDfGAZzQKNxLXgynz5QDuWWPSAZBZSHptvWRL3BjCvzUXRdKvHL2b7yGrRQcWyaqsaBCncVG7BFggS8w9snUts67BSh3EqKpXLUm5UMHfD7ZBe9GhARjbNQMLJ1QD3Spr6oMTBU6EhdB4RD8CP2xUxr2u3d6fos36PD98XS6oX8TQjLpsMwncs5DAMiD4nNnR8NBfyghGCWvCVifVwvA8B8TJxE1aiyiv2L429BCWfyzAme5sZW8rDb14NeCQHhZbtNqfXhcp2tAnaAT"
    ]
  }
'

```

Result:
```json
{"jsonrpc":"2.0","result":"2id3YC2jK9G5Wo2phDx4gJVAew8DcY5NAojnVuao8rkxwPYPe8cSwE5GzhEgJA2y8fVjDEo6iR6ykBvDxrTQrtpb","id":1}
```

### simulateTransaction

Simulate sending a transaction

#### Parameters:

- `<string>` - Transaction, as an encoded string. The transaction must have a valid blockhash, but is not required to be signed.
- `<object>` - (optional) Configuration object containing the following field:
  - `sigVerify: <bool>` - if true the transaction signatures will be verified (default: false)
  - `commitment: <string>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment) level to simulate the transaction at (default: `"max"`).
  - `encoding: <string>` - (optional) Encoding used for the transaction data. Either `"base58"` (*slow*, **DEPRECATED**), or `"base64"`. (default: `"base58"`).

#### Results:

An RpcResponse containing a TransactionStatus object The result will be an RpcResponse JSON object with `value` set to a JSON object with the following fields:

- `err: <object | string | null>` - Error if transaction failed, null if transaction succeeded. [TransactionError definitions](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
- `logs: <array | null>` - Array of log messages the transaction instructions output during execution, null if simulation failed before the transaction was able to execute (for example due to an invalid blockhash or signature verification failure)

#### Example:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "simulateTransaction",
    "params": [
      "4hXTCkRzt9WyecNzV1XPgCDfGAZzQKNxLXgynz5QDuWWPSAZBZSHptvWRL3BjCvzUXRdKvHL2b7yGrRQcWyaqsaBCncVG7BFggS8w9snUts67BSh3EqKpXLUm5UMHfD7ZBe9GhARjbNQMLJ1QD3Spr6oMTBU6EhdB4RD8CP2xUxr2u3d6fos36PD98XS6oX8TQjLpsMwncs5DAMiD4nNnR8NBfyghGCWvCVifVwvA8B8TJxE1aiyiv2L429BCWfyzAme5sZW8rDb14NeCQHhZbtNqfXhcp2tAnaAT"
    ]
  }
'
```

Result:
```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 218
    },
    "value": {
      "err": null,
      "logs": [
        "BPF program 83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri success"
      ]
    }
  },
  "id": 1
}
```

### setLogFilter

Sets the log filter on the validator

#### Parameters:

- `<string>` - the new log filter to use

#### Results:

- `<null>`

#### Example:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"setLogFilter", "params":["solana_core=debug"]}
'
```

Result:
```json
{"jsonrpc":"2.0","result":null,"id":1}
```

### validatorExit

If a validator boots with RPC exit enabled (`--enable-rpc-exit` parameter), this request causes the validator to exit.

#### Parameters:

None

#### Results:

- `<bool>` - Whether the validator exit operation was successful

#### Example:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"validatorExit"}
'

```

Result:
```json
{"jsonrpc":"2.0","result":true,"id":1}
```

## Subscription Websocket

After connecting to the RPC PubSub websocket at `ws://<ADDRESS>/`:

- Submit subscription requests to the websocket using the methods below
- Multiple subscriptions may be active at once
- Many subscriptions take the optional [`commitment` parameter](jsonrpc-api.md#configuring-state-commitment), defining how finalized a change should be to trigger a notification. For subscriptions, if commitment is unspecified, the default value is `"singleGossip"`.

### accountSubscribe

Subscribe to an account to receive notifications when the lamports or data for a given account public key changes

#### Parameters:

- `<string>` - account Pubkey, as base-58 encoded string
- `<object>` - (optional) Configuration object containing the following optional fields:
  - `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)
  - `encoding: <string>` - encoding for Account data, either "base58" (*slow*), "base64", "base64+zstd" or "jsonParsed". "jsonParsed" encoding attempts to use program-specific state parsers to return more human-readable and explicit account state data. If "jsonParsed" is requested but a parser cannot be found, the field falls back to binary encoding, detectable when the `data` field is type `<string>`.

#### Results:

- `<number>` - Subscription id \(needed to unsubscribe\)

#### Example:

Request:
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "accountSubscribe",
  "params": [
    "CM78CPUeXjn8o3yroDHxUtKsZZgoy4GPkPPXfouKNH12",
    {
      "encoding": "base64",
      "commitment": "root"
    }
  ]
}
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "accountSubscribe",
  "params": [
    "CM78CPUeXjn8o3yroDHxUtKsZZgoy4GPkPPXfouKNH12",
    {
      "encoding": "jsonParsed"
    }
  ]
}
```

Result:
```json
{"jsonrpc": "2.0","result": 23784,"id": 1}
```

#### Notification Format:

Base58 encoding:
```json
{
  "jsonrpc": "2.0",
  "method": "accountNotification",
  "params": {
    "result": {
      "context": {
        "slot": 5199307
      },
      "value": {
        "data": ["11116bv5nS2h3y12kD1yUKeMZvGcKLSjQgX6BeV7u1FrjeJcKfsHPXHRDEHrBesJhZyqnnq9qJeUuF7WHxiuLuL5twc38w2TXNLxnDbjmuR", "base58"],
        "executable": false,
        "lamports": 33594,
        "owner": "11111111111111111111111111111111",
        "rentEpoch": 635
      }
    },
    "subscription": 23784
  }
}
```

Parsed-JSON encoding:
```json
{
  "jsonrpc": "2.0",
  "method": "accountNotification",
  "params": {
    "result": {
      "context": {
        "slot": 5199307
      },
      "value": {
        "data": {
           "program": "nonce",
           "parsed": {
              "type": "initialized",
              "info": {
                 "authority": "Bbqg1M4YVVfbhEzwA9SpC9FhsaG83YMTYoR4a8oTDLX",
                 "blockhash": "LUaQTmM7WbMRiATdMMHaRGakPtCkc2GHtH57STKXs6k",
                 "feeCalculator": {
                    "lamportsPerSignature": 5000
                 }
              }
           }
        },
        "executable": false,
        "lamports": 33594,
        "owner": "11111111111111111111111111111111",
        "rentEpoch": 635
      }
    },
    "subscription": 23784
  }
}
```

### accountUnsubscribe

Unsubscribe from account change notifications

#### Parameters:

- `<number>` - id of account Subscription to cancel

#### Results:

- `<bool>` - unsubscribe success message

#### Example:

Request:
```json
{"jsonrpc":"2.0", "id":1, "method":"accountUnsubscribe", "params":[0]}

```

Result:
```json
{"jsonrpc": "2.0","result": true,"id": 1}
```

### logsSubscribe

Subscribe to transaction logging.  **UNSTABLE**

#### Parameters:

- `filter: <string>|<object>` - filter criteria for the logs to receive results by account type; currently supported:
  - "all" - subscribe to all transactions except for simple vote transactions
  - "allWithVotes" - subscribe to all transactions including simple vote transactions
  - `{ "mentions": [ <string> ] }` - subscribe to all transactions that mention the provided Pubkey (as base-58 encoded string)
- `<object>` - (optional) Configuration object containing the following optional fields:
  - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

- `<integer>` - Subscription id \(needed to unsubscribe\)

#### Example:

Request:
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "logsSubscribe",
  "params": [
    {
      "mentions": [ "11111111111111111111111111111111" ]
    },
    {
      "commitment": "max"
    }
  ]
}
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "logsSubscribe",
  "params": [ "all" ]
}
```

Result:
```json
{"jsonrpc": "2.0","result": 24040,"id": 1}
```

#### Notification Format:

Base58 encoding:
```json
{
  "jsonrpc": "2.0",
  "method": "logsNotification",
  "params": {
    "result": {
      "context": {
        "slot": 5208469
      },
      "value": {
        "signature": "5h6xBEauJ3PK6SWCZ1PGjBvj8vDdWG3KpwATGy1ARAXFSDwt8GFXM7W5Ncn16wmqokgpiKRLuS83KUxyZyv2sUYv",
        "err": null,
        "logs": [
          "BPF program 83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri success"
        ]
      }
    },
    "subscription": 24040
  }
}
```

### logsUnsubscribe

Unsubscribe from transaction logging

#### Parameters:

- `<integer>` - id of subscription to cancel

#### Results:

- `<bool>` - unsubscribe success message

#### Example:

Request:
```json
{"jsonrpc":"2.0", "id":1, "method":"logsUnsubscribe", "params":[0]}

```

Result:
```json
{"jsonrpc": "2.0","result": true,"id": 1}
```

### programSubscribe

Subscribe to a program to receive notifications when the lamports or data for a given account owned by the program changes

#### Parameters:

- `<string>` - program_id Pubkey, as base-58 encoded string
- `<object>` - (optional) Configuration object containing the following optional fields:
  - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)
  - `encoding: <string>` - encoding for Account data, either "base58" (*slow*), "base64", "base64+zstd" or "jsonParsed". "jsonParsed" encoding attempts to use program-specific state parsers to return more human-readable and explicit account state data. If "jsonParsed" is requested but a parser cannot be found, the field falls back to base64 encoding, detectable when the `data` field is type `<string>`.
  - (optional) `filters: <array>` - filter results using various [filter objects](jsonrpc-api.md#filters); account must meet all filter criteria to be included in results

#### Results:

- `<integer>` - Subscription id \(needed to unsubscribe\)

#### Example:

Request:
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "programSubscribe",
  "params": [
    "11111111111111111111111111111111",
    {
      "encoding": "base64",
      "commitment": "max"
    }
  ]
}
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "programSubscribe",
  "params": [
    "11111111111111111111111111111111",
    {
      "encoding": "jsonParsed"
    }
  ]
}
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "programSubscribe",
  "params": [
    "11111111111111111111111111111111",
    {
      "encoding": "base64",
      "filters": [
        {
          "dataSize": 80
        }
      ]
    }
  ]
}
```

Result:
```json
{"jsonrpc": "2.0","result": 24040,"id": 1}
```

#### Notification Format:

Base58 encoding:
```json
{
  "jsonrpc": "2.0",
  "method": "programNotification",
  "params": {
    "result": {
      "context": {
        "slot": 5208469
      },
      "value": {
        "pubkey": "H4vnBqifaSACnKa7acsxstsY1iV1bvJNxsCY7enrd1hq",
        "account": {
          "data": ["11116bv5nS2h3y12kD1yUKeMZvGcKLSjQgX6BeV7u1FrjeJcKfsHPXHRDEHrBesJhZyqnnq9qJeUuF7WHxiuLuL5twc38w2TXNLxnDbjmuR", "base58"],
          "executable": false,
          "lamports": 33594,
          "owner": "11111111111111111111111111111111",
          "rentEpoch": 636
        },
      }
    },
    "subscription": 24040
  }
}
```

Parsed-JSON encoding:
```json
{
  "jsonrpc": "2.0",
  "method": "programNotification",
  "params": {
    "result": {
      "context": {
        "slot": 5208469
      },
      "value": {
        "pubkey": "H4vnBqifaSACnKa7acsxstsY1iV1bvJNxsCY7enrd1hq",
        "account": {
          "data": {
             "program": "nonce",
             "parsed": {
                "type": "initialized",
                "info": {
                   "authority": "Bbqg1M4YVVfbhEzwA9SpC9FhsaG83YMTYoR4a8oTDLX",
                   "blockhash": "LUaQTmM7WbMRiATdMMHaRGakPtCkc2GHtH57STKXs6k",
                   "feeCalculator": {
                      "lamportsPerSignature": 5000
                   }
                }
             }
          },
          "executable": false,
          "lamports": 33594,
          "owner": "11111111111111111111111111111111",
          "rentEpoch": 636
        },
      }
    },
    "subscription": 24040
  }
}
```

### programUnsubscribe

Unsubscribe from program-owned account change notifications

#### Parameters:

- `<integer>` - id of account Subscription to cancel

#### Results:

- `<bool>` - unsubscribe success message

#### Example:

Request:
```json
{"jsonrpc":"2.0", "id":1, "method":"programUnsubscribe", "params":[0]}

```

Result:
```json
{"jsonrpc": "2.0","result": true,"id": 1}
```

### signatureSubscribe

Subscribe to a transaction signature to receive notification when the transaction is confirmed On `signatureNotification`, the subscription is automatically cancelled

#### Parameters:

- `<string>` - Transaction Signature, as base-58 encoded string
- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

- `integer` - subscription id \(needed to unsubscribe\)

#### Example:

Request:
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "signatureSubscribe",
  "params": [
    "2EBVM6cB8vAAD93Ktr6Vd8p67XPbQzCJX47MpReuiCXJAtcjaxpvWpcg9Ege1Nr5Tk3a2GFrByT7WPBjdsTycY9b"
  ]
}

{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "signatureSubscribe",
  "params": [
    "2EBVM6cB8vAAD93Ktr6Vd8p67XPbQzCJX47MpReuiCXJAtcjaxpvWpcg9Ege1Nr5Tk3a2GFrByT7WPBjdsTycY9b",
    {
      "commitment": "max"
    }
  ]
}
```

Result:
```json
{"jsonrpc": "2.0","result": 0,"id": 1}
```

#### Notification Format:
```bash
{
  "jsonrpc": "2.0",
  "method": "signatureNotification",
  "params": {
    "result": {
      "context": {
        "slot": 5207624
      },
      "value": {
        "err": null
      }
    },
    "subscription": 24006
  }
}
```

### signatureUnsubscribe

Unsubscribe from signature confirmation notification

#### Parameters:

- `<integer>` - subscription id to cancel

#### Results:

- `<bool>` - unsubscribe success message

#### Example:

Request:
```json
{"jsonrpc":"2.0", "id":1, "method":"signatureUnsubscribe", "params":[0]}

```

Result:
```json
{"jsonrpc": "2.0","result": true,"id": 1}
```

### slotSubscribe

Subscribe to receive notification anytime a slot is processed by the validator

#### Parameters:

None

#### Results:

- `integer` - subscription id \(needed to unsubscribe\)

#### Example:

Request:
```json
{"jsonrpc":"2.0", "id":1, "method":"slotSubscribe"}

```

Result:
```json
{"jsonrpc": "2.0","result": 0,"id": 1}
```

#### Notification Format:

```bash
{
  "jsonrpc": "2.0",
  "method": "slotNotification",
  "params": {
    "result": {
      "parent": 75,
      "root": 44,
      "slot": 76
    },
    "subscription": 0
  }
}
```

### slotUnsubscribe

Unsubscribe from slot notifications

#### Parameters:

- `<integer>` - subscription id to cancel

#### Results:

- `<bool>` - unsubscribe success message

#### Example:

Request:
```json
{"jsonrpc":"2.0", "id":1, "method":"slotUnsubscribe", "params":[0]}

```

Result:
```json
{"jsonrpc": "2.0","result": true,"id": 1}
```

### rootSubscribe

Subscribe to receive notification anytime a new root is set by the validator.

#### Parameters:

None

#### Results:

- `integer` - subscription id \(needed to unsubscribe\)

#### Example:

Request:
```json
{"jsonrpc":"2.0", "id":1, "method":"rootSubscribe"}

```

Result:
```json
{"jsonrpc": "2.0","result": 0,"id": 1}
```

#### Notification Format:

The result is the latest root slot number.

```bash
{
  "jsonrpc": "2.0",
  "method": "rootNotification",
  "params": {
    "result": 42,
    "subscription": 0
  }
}
```

### rootUnsubscribe

Unsubscribe from root notifications

#### Parameters:

- `<integer>` - subscription id to cancel

#### Results:

- `<bool>` - unsubscribe success message

#### Example:

Request:
```json
{"jsonrpc":"2.0", "id":1, "method":"rootUnsubscribe", "params":[0]}

```

Result:
```json
{"jsonrpc": "2.0","result": true,"id": 1}
```

### voteSubscribe - Unstable, disabled by default

**This subscription is unstable and only available if the validator was started with the `--rpc-pubsub-enable-vote-subscription` flag.  The format of this subscription may change in the future**

Subscribe to receive notification anytime a new vote is observed in gossip. These votes are pre-consensus therefore there is no guarantee these votes will enter the ledger.

#### Parameters:

None

#### Results:

- `integer` - subscription id \(needed to unsubscribe\)

#### Example:

Request:
```json
{"jsonrpc":"2.0", "id":1, "method":"voteSubscribe"}

```

Result:
```json
{"jsonrpc": "2.0","result": 0,"id": 1}
```

#### Notification Format:

The result is the latest vote, containing its hash, a list of voted slots, and an optional timestamp.

```json
{
  "jsonrpc": "2.0",
  "method": "voteNotification",
  "params": {
    "result": {
      "hash": "8Rshv2oMkPu5E4opXTRyuyBeZBqQ4S477VG26wUTFxUM",
      "slots": [1, 2],
      "timestamp": null
    },
    "subscription": 0
  }
}
```

### voteUnsubscribe

Unsubscribe from vote notifications

#### Parameters:

- `<integer>` - subscription id to cancel

#### Results:

- `<bool>` - unsubscribe success message

#### Example:

Request:
```json
{"jsonrpc":"2.0", "id":1, "method":"voteUnsubscribe", "params":[0]}
```

Response:
```json
{"jsonrpc": "2.0","result": true,"id": 1}
```
