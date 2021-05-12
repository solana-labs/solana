---
title: Adicione Solana a sua Exchange
---

Este guia descreve como adicionar SOL token nativo de Solana à sua Exchange de criptomoeda.

## Configuração do Node

É altamente recomendável configurar pelo menos dois nós em instâncias de alta grau/na nuvem atualizando prontamente para versões mais recentes e mantendo um olho no serviço operações com uma ferramenta de monitoramento empacotada.

Esta configuração permite você:

- para ter um gateway confiável para o cluster Solana mainnet-beta para obter dados e enviar transações de retirada
- para ter controle total sobre quantos dados de bloco histórico são mantidos
- para manter sua disponibilidade de serviço, mesmo se um nó falhar

Javier Solana exige poder de computação relativamente elevado para lidar com nossos blocos rápidos e alto TPS. Para requisitos específicos, consulte [recomendações de hardware](../running-validator/validator-reqs.md).

Para executar um nó de api:

1. [Instale as ferramentas de linha de comando Solana](../cli/install-solana-cli-tools.md)
2. Inicie o validador com pelo menos os seguintes parâmetros:

```bash
solana-validator \
  --ledger <LEDGER_PATH> \
  --entrypoint <CLUSTER_ENTRYPOINT> \
  --expected-genesis-hash <EXPECTED_GENESIS_HASH> \
  --rpc-port 8899 \
  --no-vote \
  --enable-rpc-transaction-history \
  --limit-ledger-size \
  --trusted-validator <VALIDATOR_ADDRESS> \
  --no-untrusted-rpc
```

Personalize `--ledger` com o local de armazenamento de registro desejado, e `--rpc-port` na porta que você deseja expor.

Os parâmetros `--entrypoint` e `--expected-genesis-hash` são todos específicos para o cluster que você está se juntando. [Parâmetros atuais para Mainnet Beta](../clusters.md#example-solana-validator-command-line-2)

O parâmetro `--limit-ledger-size` permite que você especifique quantos ledger [shhundred](../terminology.md#shred) seu nó retém no disco. Se você não incluir esse parâmetro, o validador manterá o livro-razão inteiro até que ele esgote espaço em disco. As tentativas de valor padrão para manter o uso do disco de registro menos de 500GB. Mais ou menos uso de disco pode ser solicitado adicionando um argumento a `--limit-ledger-size` se desejado. Verifique `solana-validator --help` no valor de limite padrão usado por `--limit-ledger-size`. Mais informações sobre selecionar um limite personalizado está [disponível aqui](https://github.com/solana-labs/solana/blob/583cec922b6107e0f85c7e14cb5e642bc7dfb340/core/src/ledger_cleanup_service.rs#L15-L26).

Especificar um ou mais parâmetros `--trusted-validator` podem protegê-lo de inicialização de um instantâneo malicioso. [Mais sobre o valor de inicialização com validadores confiáveis](../running-validator/validator-start.md#trusted-validators)

Parâmetros opcionais a considerar:

- `--private-rpc` impede que sua porta RPC seja publicada para uso por outros nós
- `--rpc-bind-address` permite que você especifique um endereço IP diferente para vincular a porta RPC

### Reinicia e Monitoramento automático

Recomendamos configurar cada um dos seus nós para reiniciar automaticamente ao sair, para garantir que você perca o mínimo de dados possível. Executar o software da solana como um serviço de sistema de é uma ótima opção.

For monitoring, we provide [`solana-watchtower`](https://github.com/solana-labs/solana/blob/master/watchtower/README.md), which can monitor your validator and detect with the `solana-validator` process is unhealthy. Ela pode ser configurada diretamente para alertá-lo via Slack, Telegram, Discord ou Twillio. Para obter detalhes, execute `solana-watchtorre --help`.

```bash
solana-watchtorre --validator-identity <YOUR VALIDATOR IDENTITY>
```

#### Anúncios de Novo Software de Lançamento

Nós lançamos novo software frequentemente (cerca de 1 lançamento / semana). Às vezes, as versões mais recentes incluem alterações de protocolo incompatíveis, que requer uma atualização atempada do software para evitar erros no processamento de blocos.

Nossos anúncios oficiais de lançamento para todos os tipos de lançamentos (normal e segurança) são comunicados por meio de um canal de discórdia chamado [`#mb-announcement`](https://discord.com/channels/428295358100013066/669406841830244375) (`mb` stands for `mainnet-beta`).

Curtir validadores acionados, nós esperamos que todos os validadores operados em troca sejam atualizados no mais breve prazo possível, dentro de um dia útil ou dois após o lançamento normal anúncio. Para libertações relacionadas com a segurança, poderá ser necessária uma acção mais urgente.

### Continuidade do alegre

Por padrão, cada um dos seus nós inicializará a partir de um snapshot fornecido por um de seus validadores confiáveis. Este instantâneo reflete o estado atual da cadeia, mas não contém o registro histórico completo. Se uma das suas botas e um sair de uma nova foto, pode haver uma lacuna no livro-razão nesse nó. Em para evitar esse problema adicione o parâmetro `--no-snapshot-fetch` ao comando `solana-validator` para receber dados históricos de ledger em vez de um snapshot .

Não passe o parâmetro `--no-snapshot-fetch` na sua inicialização inicial, pois não é possível inicializar o nó a partir do bloco de gênesis. Em vez disso, inicialize a partir de um snapshot primeiro e depois adicione o parâmetro `--no-snapshot-fetch` para reinicializações.

É importante notar que a quantidade de livro razão histórico disponível para seus nós do resto da rede é limitada a qualquer momento no tempo. Uma vez operacional se seus validadores experimentarem inatividade significante, eles podem não ser capazes de alcançar a rede e precisarem baixar um novo instantâneo de um validador confiável. Ao fazer isso, os seus validadores agora terão uma lacuna nos seus dados históricos que não podem ser preenchidos.

### Minimizando a Exposição da Porta do Validador

O validador requer que várias portas UDP e TCP estejam abertas para o tráfego de entrada de todos os outros validadores Javier Solana. Embora este seja o modo mais eficiente de operação, e é fortemente recomendado, É possível restringir o validador para exigir apenas o tráfego de entrada de um outro validador Solana.

Primeiro adicione o argumento `--restricted-repair-only-mode`. Isto fará com que o validador opere em um modo restrito onde não receberá pushes de o resto dos validadores e, em vez disso, precisará sondar continuamente outros validadores para blocos. O validador só transmitirá pacotes UDP para outros validadores usando os _Gossip_ e _ServeR_ ("serve repair") ports, e somente receber pacotes UDP em suas portas _Gossip_ e _Reparar_.

A porta _Gossip_ é bidirecional e permite que seu validador permaneça em contato com o resto do cluster. Seu validador transmite no _ServeR_ para fazer pedidos de reparo para obter novos blocos do resto da rede, desde que a Turbina está desativada agora. Seu validador então receberá respostas de reparo na porta _Repair_ de outros validadores.

Para restringir ainda mais o validador para solicitar apenas blocos de um ou mais validadores, Primeiro, determine a chave de identidade desse validador e adicione os argumentos do `--gossip-pull-validator PUBKEY --repair-validator PUBKEY` para cada PUBKEY. Isso fará com que seu validador seja um drenão de recurso em cada validador que você adicionar, então por favor faça isso com moderação e somente depois de consultar o validador de alvo de.

Seu validador agora só deve estar se comunicando com os validadores explicitamente listados e apenas no _Gossip_, _Reparo_ e _ServeR_ portas.

## Configurando contas de depósito

Contas Solana não requerem nenhuma inicialização on-chain; uma vez que contêm alguns SOL, elas existem. Para criar uma conta de depósito para a sua corretora, simplesmente gerar um par de chaves Solana usando qualquer uma das nossas [ferramentas de carteira](../wallet-guide/cli.md).

Recomendamos usar uma conta de depósito exclusiva para cada um dos seus usuários.

Contas Solana são cobradas [aluguel](developing/programming-model/accounts.md#rent) de criação e uma vez a cada epoch, mas podem ficar isentos de aluguel se tiverem 2 anos de aluguel em SOL. Para encontrar o saldo mínimo de isenção de aluguel para suas contas de depósito, consulte o [`getMinimumBalanceForRentExemption` endpoint](developing/clients/jsonrpc-api.md#getminimumbalanceforrentexemption):

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc": "2.0","id":1, "method":"getMinimumBalanceForRentExemption","params":[0]}' localhost:8899

{"jsonrpc":"2.0","result":890880,"id":1}
```

### Contas offline

Você pode querer manter as chaves para uma ou mais contas de coleta offline para uma maior segurança. Em caso afirmativo, você precisará mover o SOL para contas quentes usando nossos [métodos offline](../offline-signing.md).

## Escutando Depósitos

Quando um usuário quiser depositar SOL em sua Exchange, instrua-o a enviar uma transferência para o endereço de depósito apropriado.

### Enquete para blocos

Para controlar todas as contas de depósito da sua Exchange, enquete por cada bloco confirmado e inspecionar por endereços de juros, usando o serviço JSON-RPC do seu Servidor API.

- Para identificar quais blocos estão disponíveis, envie uma solicitação [`getConfirmedBlocks`](developing/clients/jsonrpc-api.md#getconfirmedblocks), passando o último bloco que você já foi processado como o parâmetro inicial:

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc": "2.0","id":1, "method":"getMinimumBalanceForRentExemption","params":[0]}' localhost:8899

{"jsonrpc":"2.0","result":890880,"id":1}
```

Nem todos os espaços produzem um bloco, então pode haver lacunas na sequência de inteiros.

- Para cada bloco, solicite seu conteúdo com uma solicitação [`getConfirmedBlock`](developing/clients/jsonrpc-api.md#getconfirmedblock):

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc": "2.0","id":1,"method":"getConfirmedBlock","params":[5, "json"]}' localhost:8899

{
  "jsonrpc": "2.0",
  "result": {
    "blockhash": "2WcrsKSVANoe6xQHKtCcqNdUpCQPQ3vb6QTgi1dcE2oL",
    "parentSlot": 4,
    "previousBlockhash": "7ZDoGW83nXgP14vnn9XhGSaGjbuLdLWkQAoUQ7pg6qDZ",
    "rewards": [],
    "transactions": [
      {
        "meta": {
          "err": null,
          "fee": 5000,
          "postBalances": [
            2033973061360,
            218099990000,
            42000000003
          ],
          "preBalances": [
            2044973066360,
            207099990000,
            42000000003
          ],
          "status": {
            "Ok": null
          }
        },
        "transaction": {
          "message": {
            "accountKeys": [
              "Bbqg1M4YVVfbhEzwA9SpC9FhsaG83YMTYoR4a8oTDLX",
              "47Sbuv6jL7CViK9F2NMW51aQGhfdpUu7WNvKyH645Rfi",
              "11111111111111111111111111111111"
            ],
            "header": {
              "numReadonlySignedAccounts": 0,
              "numReadonlyUnsignedAccounts": 1,
              "numRequiredSignatures": 1
            },
            "instructions": [
              {
                "accounts": [
                  0,
                  1
                ],
                "data": "3Bxs3zyH82bhpB8j",
                "programIdIndex": 2
              }
            ],
            "recentBlockhash": "7GytRgrWXncJWKhzovVoP9kjfLwoiuDb3cWjpXGnmxWh"
          },
          "signatures": [
            "dhjhJp2V2ybQGVfELWM1aZy98guVVsxRCB5KhNiXFjCBMK5KEyzV8smhkVvs3xwkAug31KnpzJpiNPtcD5bG1t6"
          ]
        }
      }
    ]
  },
  "id": 1
}
```

The `preBalances` and `postBalances` fields allow you to track the balance changes in every account without having to parse the entire transaction. They list the starting and ending balances of each account in [lamports](../terminology.md#lamport), indexed to the `accountKeys` list. For example, if the deposit address if interest is `47Sbuv6jL7CViK9F2NMW51aQGhfdpUu7WNvKyH645Rfi`, this transaction represents a transfer of 218099990000 - 207099990000 = 11000000000 lamports = 11 SOL

If you need more information about the transaction type or other specifics, you can request the block from RPC in binary format, and parse it using either our [Rust SDK](https://github.com/solana-labs/solana) or [Javascript SDK](https://github.com/solana-labs/solana-web3.js).

### Address History

You can also query the transaction history of a specific address. This is generally _not_ a viable method for tracking all your deposit addresses over all slots, but may be useful for examining a few accounts for a specific period of time.

- Send a [`getConfirmedSignaturesForAddress2`](developing/clients/jsonrpc-api.md#getconfirmedsignaturesforaddress2) request to the api node:

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc": "2.0","id":1,"method":"getConfirmedSignaturesForAddress2","params":["6H94zdiaYfRfPfKjYLjyr2VFBg6JHXygy84r3qhc3NsC", {"limit": 3}]}' localhost:8899

{
  "jsonrpc": "2.0",
  "result": [
    {
      "err": null,
      "memo": null,
      "signature": "35YGay1Lwjwgxe9zaH6APSHbt9gYQUCtBWTNL3aVwVGn9xTFw2fgds7qK5AL29mP63A9j3rh8KpN1TgSR62XCaby",
      "slot": 114
    },
    {
      "err": null,
      "memo": null,
      "signature": "4bJdGN8Tt2kLWZ3Fa1dpwPSEkXWWTSszPSf1rRVsCwNjxbbUdwTeiWtmi8soA26YmwnKD4aAxNp8ci1Gjpdv4gsr",
      "slot": 112
    },
    {
      "err": null,
      "memo": null,
      "signature": "dhjhJp2V2ybQGVfELWM1aZy98guVVsxRCB5KhNiXFjCBMK5KEyzV8smhkVvs3xwkAug31KnpzJpiNPtcD5bG1t6",
      "slot": 108
    }
  ],
  "id": 1
}
```

- For each signature returned, get the transaction details by sending a [`getConfirmedTransaction`](developing/clients/jsonrpc-api.md#getconfirmedtransaction) request:

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc": "2.0","id":1,"method":"getConfirmedTransaction","params":["dhjhJp2V2ybQGVfELWM1aZy98guVVsxRCB5KhNiXFjCBMK5KEyzV8smhkVvs3xwkAug31KnpzJpiNPtcD5bG1t6", "json"]}' localhost:8899

// Result
{
  "jsonrpc": "2.0",
  "result": {
    "slot": 5,
    "transaction": {
      "message": {
        "accountKeys": [
          "Bbqg1M4YVVfbhEzwA9SpC9FhsaG83YMTYoR4a8oTDLX",
          "47Sbuv6jL7CViK9F2NMW51aQGhfdpUu7WNvKyH645Rfi",
          "11111111111111111111111111111111"
        ],
        "header": {
          "numReadonlySignedAccounts": 0,
          "numReadonlyUnsignedAccounts": 1,
          "numRequiredSignatures": 1
        },
        "instructions": [
          {
            "accounts": [
              0,
              1
            ],
            "data": "3Bxs3zyH82bhpB8j",
            "programIdIndex": 2
          }
        ],
        "recentBlockhash": "7GytRgrWXncJWKhzovVoP9kjfLwoiuDb3cWjpXGnmxWh"
      },
      "signatures": [
        "dhjhJp2V2ybQGVfELWM1aZy98guVVsxRCB5KhNiXFjCBMK5KEyzV8smhkVvs3xwkAug31KnpzJpiNPtcD5bG1t6"
      ]
    },
    "meta": {
      "err": null,
      "fee": 5000,
      "postBalances": [
        2033973061360,
        218099990000,
        42000000003
      ],
      "preBalances": [
        2044973066360,
        207099990000,
        42000000003
      ],
      "status": {
        "Ok": null
      }
    }
  },
  "id": 1
}
```

## Sending Withdrawals

To accommodate a user's request to withdraw SOL, you must generate a Solana transfer transaction, and send it to the api node to be forwarded to your cluster.

### Synchronous

Sending a synchronous transfer to the Solana cluster allows you to easily ensure that a transfer is successful and finalized by the cluster.

Solana's command-line tool offers a simple command, `solana transfer`, to generate, submit, and confirm transfer transactions. By default, this method will wait and track progress on stderr until the transaction has been finalized by the cluster. If the transaction fails, it will report any transaction errors.

```bash
solana transfer <USER_ADDRESS> <AMOUNT> --keypair <KEYPAIR> --url http://localhost:8899
```

The [Solana Javascript SDK](https://github.com/solana-labs/solana-web3.js) offers a similar approach for the JS ecosystem. Use the `SystemProgram` to build a transfer transaction, and submit it using the `sendAndConfirmTransaction` method.

### Asynchronous

For greater flexibility, you can submit withdrawal transfers asynchronously. In these cases, it is your responsibility to verify that the transaction succeeded and was finalized by the cluster.

**Note:** Each transaction contains a [recent blockhash](developing/programming-model/transactions.md#blockhash-format) to indicate its liveness. It is **critical** to wait until this blockhash expires before retrying a withdrawal transfer that does not appear to have been confirmed or finalized by the cluster. Otherwise, you risk a double spend. See more on [blockhash expiration](#blockhash-expiration) below.

First, get a recent blockhash using the [`getFees` endpoint](developing/clients/jsonrpc-api.md#getfees) or the CLI command:

```bash
solana fees --url http://localhost:8899
```

In the command-line tool, pass the `--no-wait` argument to send a transfer asynchronously, and include your recent blockhash with the `--blockhash` argument:

```bash
solana transfer <USER_ADDRESS> <AMOUNT> --no-wait --blockhash <RECENT_BLOCKHASH> --keypair <KEYPAIR> --url http://localhost:8899
```

You can also build, sign, and serialize the transaction manually, and fire it off to the cluster using the JSON-RPC [`sendTransaction` endpoint](developing/clients/jsonrpc-api.md#sendtransaction).

#### Transaction Confirmations & Finality

Get the status of a batch of transactions using the [`getSignatureStatuses` JSON-RPC endpoint](developing/clients/jsonrpc-api.md#getsignaturestatuses). The `confirmations` field reports how many [confirmed blocks](../terminology.md#confirmed-block) have elapsed since the transaction was processed. If `confirmations: null`, it is [finalized](../terminology.md#finality).

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0", "id":1, "method":"getSignatureStatuses", "params":[["5VERv8NMvzbJMEkV8xnrLkEaWRtSz9CosKDYjCJjBRnbJLgp8uirBgmQpjKhoR4tjF3ZpRzrFmBV6UjKdiSZkQUW", "5j7s6NiJS3JAkvgkoc18WVAsiSaci2pxB2A6ueCJP4tprA2TFg9wSyTLeYouxPBJEMzJinENTkpA52YStRW5Dia7"]]}' http://localhost:8899

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
        }
      },
      {
        "slot": 48,
        "confirmations": null,
        "err": null,
        "status": {
          "Ok": null
        }
      }
    ]
  },
  "id": 1
}
```

#### Blockhash Expiration

When you request a recent blockhash for your withdrawal transaction using the [`getFees` endpoint](developing/clients/jsonrpc-api.md#getfees) or `solana fees`, the response will include the `lastValidSlot`, the last slot in which the blockhash will be valid. You can check the cluster slot with a [`getSlot` query](developing/clients/jsonrpc-api.md#getslot); once the cluster slot is greater than `lastValidSlot`, the withdrawal transaction using that blockhash should never succeed.

You can also doublecheck whether a particular blockhash is still valid by sending a [`getFeeCalculatorForBlockhash`](developing/clients/jsonrpc-api.md#getfeecalculatorforblockhash) request with the blockhash as a parameter. If the response value is null, the blockhash is expired, and the withdrawal transaction should never succeed.

### Validating User-supplied Account Addresses for Withdrawals

As withdrawals are irreversible, it may be a good practice to validate a user-supplied account address before authorizing a withdrawal in order to prevent accidental loss of user funds.

The address of a normal account in Solana is a Base58-encoded string of a 256-bit ed25519 public key. Not all bit patterns are valid public keys for the ed25519 curve, so it is possible to ensure user-supplied account addresses are at least correct ed25519 public keys.

#### Java

Here is a Java example of validating a user-supplied address as a valid ed25519 public key:

The following code sample assumes you're using the Maven.

`pom.xml`:

```xml
<repositories>
  ...
  <repository>
    <id>spring</id>
    <url>https://repo.spring.io/libs-release/</url>
  </repository>
</repositories>...

<dependencies>
  ...
  <dependency>
      <groupId>io.github.novacrypto</groupId>
      <artifactId>Base58</artifactId>
      <version>0.1.3</version>
  </dependency>
  <dependency>
      <groupId>cafe.cryptography</groupId>
      <artifactId>curve25519-elisabeth</artifactId>
      <version>0.1.0</version>
  </dependency>
<dependencies>
```

```java
importar io.github.novacrypto.base58.Base58;
importar cafe.cryptography.curve25519. ompressedEdwardsY;

public class PubkeyValidator
{
    public boolean verifyPubkey(String userProvidedPubkey)
    {
        try {
            return _verifyPubkeyInternal(userProvidedPubkey);
        } catch (Exceção) {
            return false;
        }
    }

    booleano estático público _verifyPubkeyInternal(String maybePubkey) lança Exceção
    {
        byte[] bytes = Base58. ase58Decode(maybePubkey);
        return !(new CompressedEdwardsY(bytes).decompress().isSmallOrder();
    }
}
```

## Suporte o Padrão de Token SPL

[Token SPL](https://spl.solana.com/token) é o padrão para criação de tokens encapsulado/sintético e troca na blockchain Solana.

O fluxo de trabalho do Token SPL é semelhante ao dos tokens nativos de SOL, mas existem algumas diferenças que serão discutidas nesta seção.

### Token Mints

Cada _tipo_ do SPL Token é declarado criando uma conta _mint_. Esta conta armazena metadados que descrevem recursos do token, como a oferta, número de decimais e várias autoridades com controle sobre a mentalidade. Cada conta SPL Token faz referência a sua menta associada e só pode interagir com Tokens SPL desse tipo.

### Instalando a `spl-token` Ferramenta CLI

Contas SPL Token são necessárias e modificadas usando o utilitário da linha de comando `spl-token`. Os exemplos fornecidos nesta seção dependem de ter ele instalado no sistema local.

`spl-token` é distribuído a partir de Rust [crates.io](https://crates.io/crates/spl-token) via o Rust `cargo` utilitário de linha de comando. A última versão do `de carga` pode ser instalada usando um útil one-liner para a sua plataforma em [rustup.rs](https://rustup.rs). Assim que o `cargo` for instalado, o `spl-token` pode ser obtido com o seguinte comando:

```
cargo instalar spl-token-cli
```

Você pode verificar a versão instalada para verificar

```
token spl-token --version
```

O que deve resultar em algo como

```text
spl-token-cli 2.0.1
```

### Criação de conta

Contas SPL Token transportam requisitos adicionais que o Programa de Sistema nativo contas não:

1. Contas de token SPL devem ser criadas antes que uma quantidade de tokens possa ser depositada. Contas de token podem ser criadas explicitamente com o comando `spl-token create-account` , ou implicitamente pela transferência de `spl-token --fund-recipient . .` comando.
1. As contas SPL Token devem permanecer [isentas de aluguel](developing/programming-model/accounts.md#rent-exemption) pela duração da sua existência e portanto requer que uma pequena quantidade de tokens nativos SOL seja depositada na criação da conta. Para contas SPL Token v2, este valor é 0.00203928 SOL (2.039,280 lamports).

#### Linha de comando

Para criar uma conta SPL Token com as seguintes propriedades:

1. Associado com a menta determinada
1. Possuído pelo keypair da conta de financiamento

```
criar-conta de criação-token <TOKEN_MINT_ADDRESS>
```

#### Exemplo

```
$ spl-token create-account AkUFCWTXb3w9nY2n6SFJvBV6VwvFUCe4KBMCcgLsa2ir
Creating account 6VzWGL51jLebvnDifvcuEDec17sK6Wupi4gYhm5RzfkV
Signature: 4JsqZEPra2eDTHtHpB4FMWSfk3UgcCVmkKkP7zESZeMrKmFFkDkNd91pKP3vPVVZZPiu5XxyJwS73Vi5WsZL88D7
```

Ou para criar uma conta SPL Token com um par de chave específico:

```
$ solana-keygen new -o token-account.json
$ spl-token create-account AkUFCWTXb3w9nY2n6SFJvBV6VwvFUCe4KBMCcgLsa2ir token-account.json
Creating account 6VzWGL51jLebvnDifvcuEDec17sK6Wupi4gYhm5RzfkV
Signature: 4JsqZEPra2eDTHtHpB4FMWSfk3UgcCVmkKkP7zESZeMrKmFFkDkNd91pKP3vPVVZZPiu5XxyJwS73Vi5WsZL88D7
```

### Verificando Saldo de uma Conta

#### Linha de comando

```
saldo de solana <TOKEN_ACCOUNT_ADDRESS>
```

#### Exemplo

```
$ solana balance 6VzWGL51jLebvnDifvcuEDec17sK6Wupi4gYhm5RzfkV
0
```

### Transferências de token

A conta de origem de uma transferência é a conta token real que contém o valor de.

O endereço do destinatário no entanto pode ser uma conta de carteira normal. Se um associado conta token para a casa da moeda fornecida ainda não existe para essa carteira, o transferência irá criá-lo, desde que o argumento `--fund-receiver` como forneceu.

#### Linha de comando

```
transferência spl-token <SENDER_ACCOUNT_ADDRESS> <AMOUNT> <RECIPIENT_WALLET_ADDRESS> --fund-recipient
```

#### Exemplo

```
$ spl-token transfer 6B199xxzw3PkAm25hGJpjj3Wj3WNYNHzDAnt1tEqg5BN 1 6VzWGL51jLebvnDifvcuEDec17sK6Wupi4gYhm5RzfkV
Transfer 1 tokens
  Sender: 6B199xxzw3PkAm25hGJpjj3Wj3WNYNHzDAnt1tEqg5BN
  Recipient: 6VzWGL51jLebvnDifvcuEDec17sK6Wupi4gYhm5RzfkV
Signature: 3R6tsog17QM8KfzbcbdP4aoMfwgo6hBggJDVy7dZPVmH2xbCWjEj31JKD53NzMrf25ChFjY7Uv2dfCDq4mGFFyAj
```

### Depósito

Uma vez que cada par `(user, mint)` requer uma conta separada na cadeia, é recomendado que uma Exchange crie lotes de contas de token com antecedência e os atribua aos usuários a pedido. Essas contas devem ser propriedade de chaves-chave controladas por troca.

O monitoramento para as transações de depósito deve seguir o [método de consulta de bloco](#poll-for-blocks) descrito acima. Each new block should be scanned for successful transactions issuing SPL Token [Transfer](https://github.com/solana-labs/solana-program-library/blob/096d3d4da51a8f63db5160b126ebc56b26346fc8/token/program/src/instruction.rs#L92) or [Transfer2](https://github.com/solana-labs/solana-program-library/blob/096d3d4da51a8f63db5160b126ebc56b26346fc8/token/program/src/instruction.rs#L252) instructions referencing user accounts, then querying the [token account balance](developing/clients/jsonrpc-api.md#gettokenaccountbalance) updates.

[Considerações](https://github.com/solana-labs/solana/issues/12318) estão sendo feitas para superar os campos `pré-saldos` e `pós-Saldo` status de transação metadados para incluir transferências de saldo SPL token.

### Retirar

O endereço de retirada que um usuário fornece deve ser o mesmo endereço usado para retirada regular SOL.

Antes de realizar uma transferência [de retirada](#token-transfers), a troca deve verificar o endereço como [descrito acima](#validating-user-supplied-account-addresses-for-withdrawals).

A partir do endereço de retirada, o token associado conta para a casa da moeda correta determinada e a transferência emitida para essa conta. Note que é possível que a conta de token associada ainda não existe, em que ponto a troca deve financiar a conta em nome do usuário. Para SPL Token v2 contas, o financiamento da conta de saque exigirá 0,00203928 SOL (2.039.280 lamports).

Modelo `de transferência spl-token` comando para uma retirada:

```
$ transferência spl-token <exchange token account> <withdrawal amount> <withdrawal address> --fund-recipient
```

### Outras Considerações

#### Autoridade de congelamento

Por razões de conformidade regulamentar, uma entidade emissora de token SPL pode opcionalmente escolher manter a "Autoridade" sobre todas as contas criadas em associação com sua mentalidade. Isto permite que [bloqueiem](https://spl.solana.com/token#freezing-accounts) os ativos em uma determinada conta, tornando a conta inutilizável até o descongelamento. Se este recurso estiver em uso, a pubkey da autoridade congelada será registrada na conta de SPL Token.

## Testando a Integração

Certifique-se de testar seu fluxo de trabalho completo em Solana devnet e testnet [clusters](../clusters.md) antes de mudar para produção no mainnet-beta. Devnet é o mais aberto e flexível, e ideal para o desenvolvimento inicial, enquanto testnet oferece uma configuração de cluster mais realista. Tanto devnet quanto testnet suportam uma torneira, execute `solana airdrop 10` para obter algum devnet ou testnet SOL para desenvolvimento e teste.
