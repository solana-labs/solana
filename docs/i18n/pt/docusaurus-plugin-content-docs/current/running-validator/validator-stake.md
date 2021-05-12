---
title: Staking
---

**Por padrão, seu validador não terá participação.** Isso significa que será inelegível para se tornar líder.

## Monitorando captura

Para delegar stake, primeiro certifique-se de que o validador está executando e tenha chegado ao cluster. Pode levar algum tempo para recuperar o atraso após a inicialização do seu validador. Use o comando `capchup` para monitorar seu validador através deste processo:

```bash
solana catchup ~/validator-keypair.json
```

Até que seu validador tenha pegado, ele não será capaz de votar com sucesso e participação não pode ser delegada a ele.

Também se você encontrar o slot do cluster que está avançando mais rápido do que o seu, você provavelmente nunca irá recuperar o atraso. Isso normalmente implica em algum tipo de problema de rede entre seu validador e o resto do cluster.

## Criar Locação de Chaves

Se você ainda não fez isso, crie um par de chaves para staing. Se você tiver concluído esta etapa, você deve ver a pasta "validator-stake-keypair.json" em sua pasta de execução Solar.

```bash
solana-keygen new -o ~/validator-stake-keypair.json
```

## Delegate Stake

Agora delege 1 SOL ao seu validador criando primeiro sua conta de participação:

```bash
solana create-stake-account ~/validator-stake-keypair.json 1
```

e, em seguida, delegando essa participação ao seu validador:

```bash
solana delegate-stake ~/validator-stake-keypair.json ~/vote-account-keypair.json
```

> Não delege seu SOL restante, pois o seu validador usará esses tokens para votar.

Implantações podem ser re-delegadas para outro nó a qualquer momento com o mesmo comando, mas apenas uma re-delegação é permitida por etapa:

```bash
solana delegate-stake ~/validator-stake-keypair.json ~/some-other-vote-account-keypair.json
```

Supondo que o nó esteja votando, agora você está usando e gerando recompensas do validador. Recompensas são pagas automaticamente nas fronteiras de época.

As lâmpadas de recompensas obtidas são divididas entre sua conta de participação e a conta de voto de acordo com a taxa de comissão definida na conta do voto. Recompensas só podem ser obtidas enquanto o validador estiver instalado e em execução. Além disso, uma vez estampado, o validador da se torna uma parte importante da rede. Para remover com segurança um validador da rede, primeiro desative seu stake.

Ao final de cada slot, espera-se que um validador envie uma transação de voto. Essas transações de voto são pagas por lâmpadas da conta de uma identidade de um validador.

Essa é uma transação normal então será aplicada a taxa de transação padrão. O intervalo de taxa de transação é definido pelo bloco Gênesis. A taxa real irá flutuar baseado na carga da transação. Você pode determinar a taxa atual através da [API RPC "getRecentBlockhash"](developing/clients/jsonrpc-api.md#getrecentblockhash) antes de enviar uma transação.

Saiba mais sobre as [taxas de transação aqui](../implemented-proposals/transaction-fees.md).

## Locação do validador

Para combater vários ataques de consenso, novas delegações de participação estão sujeitas a um [período de aquecimento](/staking/stake-accounts#delegation-warmup-and-cooldown).

Monitore o stake do validador por:

- Veja sua conta de voto:`solana vote-account ~/vote-account-keypair.json` Isso exibe o estado atual de todos os votos que o validador enviou à rede.
- Veja sua conta de participação, as preferências de delegação e os detalhes de suas partes interessadas:`solana stake-account ~/validator-stake-keypair.json`
- `validadores de solana` mostram a atual participação ativa de todos os validadores, incluindo o seu
- `Solana stake-history` mostra a história do aquecimento da estaca e o tempo de espera nos últimos tempos
- Procure por mensagens de log no seu validador indicando seu próximo slot de líder: `[2019-09-27T20:16:00.319721164Z INFO solana_core::replay_stage] <VALIDATOR_IDENTITY_PUBKEY> votou e reiniciou PoH na altura de marcação ####. Meu próximo slot de líder é ####`
- Uma vez que sua estaca estiver aquecida, você verá um saldo de participação listado para o seu validador executando os `validadores da solana`

## Monitore Seu Validador Implantado

Confirme seu validador se torna um [líder](../terminology.md#leader)

- Depois que o validador for recuperado, use o comando `balanceamento da solana` para monitorar os ganhos enquanto seu validador é selecionado como líder e coleta taxas de transação
- Solana oferece uma série de métodos JSON-RPC úteis para retornar informações sobre a rede e a participação do seu validador. Faça uma solicitação usando curl \(ou outro cliente http à sua escolha\), especificando o método desejado em dados JSON-RPC. Por exemplo:

```bash
  // Solicitação
  curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getEpochInfo"}' http://localhost:8899

  // Resultado
  {"jsonrpc":"2.0","result":{"epoch":3,"slotIndex":126,"slotsInEpoch":256},"id":1}
```

Métodos JSON-RPC úteis:

- `getEpochInfo`[Um período](../terminology.md#epoch) é a hora, ou seja, número de [slots](../terminology.md#slot)para o qual uma agenda [líder](../terminology.md#leader-schedule) é válida. Isso irá lhe dizer qual é a época atual e o quão longe ele está no cluster.
- `getVoteAccounts` This will tell you how much active stake your validator currently has. A % of the validator's stake is activated on an epoch boundary. You can learn more about staking on Solana [here](../cluster/stake-delegation-and-rewards.md).
- `getLeaderSchedule` At any given moment, the network expects only one validator to produce ledger entries. The [validator currently selected to produce ledger entries](../cluster/leader-rotation.md#leader-rotation) is called the “leader”. This will return the complete leader schedule \(on a slot-by-slot basis\) for currently activated stake, the identity pubkey will show up 1 or more times here.

## Deactivating Stake

Before detaching your validator from the cluster, you should deactivate the stake that was previously delegated by running:

```bash
solana deactivate-stake ~/validator-stake-keypair.json
```

Stake is not deactivated immediately and instead cools down in a similar fashion as stake warm up. Your validator should remain attached to the cluster while the stake is cooling down. While cooling down, your stake will continue to earn rewards. Only after stake cooldown is it safe to turn off your validator or withdraw it from the network. Cooldown may take several epochs to complete, depending on active stake and the size of your stake.

Note that a stake account may only be used once, so after deactivation, use the cli's `withdraw-stake` command to recover the previously staked lamports.
