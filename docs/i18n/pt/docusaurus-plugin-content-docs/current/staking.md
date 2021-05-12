---
title: Staking de Solana
---

_Nota antes de ler: Todas as referências a aumentos em valores são em termos absolutos de em relação ao balanço de SOL. Este documento não faz nenhuma sugestão sobre o valor monetário da SOL a qualquer momento._

Faça Staking de seus tokens de SOL no Solana é a melhor maneira que você pode ajudar a proteger a rede blockchain do mundo com maior desempenho de, e [receba recompensas](implemented-proposals/staking-rewards.md) por isso! Inflação e recompensas de rede _NÃO_ atualmente estão ativadas na rede Beta principal de Solana, mas podem ser ativadas no futuro.

Solana é uma rede Proof-of-Stake (PoS) com delegações, o que significa que qualquer um que tenha tokens SOL pode optar por delegar alguns de seus SOL para um ou mais validadores, que processam transações e executam a rede.

Participação de delegação é um modelo financeiro de partilha de risco que pode fornecer retornos aos detentores de tokens delegados por um longo período. Isso é conseguido alinhando os incentivos financeiros dos detentores de tokens (delegadores) e os validadores a quem eles delegam.

Quanto mais stake um validador delegou a eles, mais frequentemente esse validador é escolhido para escrever novas transações para o ledger. Quanto mais transações o validador escreve, mais recompensas eles e seus delegados ganham. Validadores que configuram seus sistemas para ser capaz de processar mais transações de cada vez não só ganham proporcionalmente mais recompensas por isso, eles também mantêm a rede rodando o mais rápido e suavemente possível.

Validadores incorrem em custos, executando e mantendo seus sistemas, e isso é passado para os delegados, sob a forma de uma taxa recebida em relação a porcentagem de recompensas recebidas. Esta taxa é conhecida como uma comissão de \*\*. À medida que os validadores ganham mais recompensas, a participação é delegada a eles, eles podem competir uns com os outros para oferecer a comissão mais baixa por seus serviços, a fim de atrair mais participações delegadas.

Há um risco de perda de tokens quando staking, através de um processo conhecido como _cortando_. Inflação e recompensas de rede _NÃO_ atualmente estão ativadas na rede Beta principal de Solana, mas podem ser ativadas no futuro. Cortar envolve a remoção e destruição automática de de uma porção da participação delegada de um validador na resposta ao comportamento malicioso intencional, tais como criar transações inválidas ou censurar certos tipos de transações ou participantes da rede. Se um validador for cortado, todos os titulares de tokens que delegaram participação para aquele validador perderão uma porção de sua delegação. Enquanto isso significa uma perda imediata para o titular do token, também é uma perda de futuras recompensas para o validador devido a sua delegação total reduzida.

É o objetivo das recompensas da rede e do corte para alinhar os incentivos financeiros dos validadores e dos seus titulares, o que, por sua vez, ajuda a manter a rede segura, robusta e performando no seu melhor.

_Nota: Recompensas de rede para participantes e validadores não estão atualmente ativadas no Beta Principal._

_Nota: O Slashing não está ativado no Mainnet Beta neste momento._

## Como faço para fazer deploy dos meus tokens de SOL?

Para colocar tokens em um Solana, você primeiro precisará transferir alguns SOL em uma carteira que suporta staking, em seguida, siga as etapas ou instruções fornecidas pela carteira para criar uma conta de participação e delegar sua participação. Diferentes carteiras irão variar ligeiramente em seu processo para isso, mas a descrição geral está abaixo.

#### Carteiras Suportadas

Operações de staking são suportadas pelas seguintes soluções de carteira:

- SolFlare.com em conjunto com um arquivo keystore ou um Ledger Nano. Confira nosso [guia para usar o SolFlare](wallet-guide/solflare.md) para detalhes.

- Ferramentas de linha de comando Solana podem executar todas as operações de staking em conjunto com uma carteira de arquivo de keypair gerada por CLI, uma carteira de papel, ou com um Ledger Nano conectado. [Comandos de presença usando as Ferramentas de Linha de Comando Solana,](cli/delegate-stake.md).

#### Criar uma Conta Stake

Uma conta de stake é um tipo diferente de conta de um endereço de carteira que é usado para simplesmente enviar e receber tokens SOL para outros endereços. Se você recebeu SOL em um endereço de carteira que você controla, você pode usar alguns dos esses tokens para criar e financiar uma nova conta de aposta, que terá um endereço do que a carteira que você usou para criá-lo. Dependendo de qual carteira você está usando as etapas para criar uma conta de participação pode variar ligeiramente. Nem todas as carteiras suportam contas stake [Carteiras Suportadas](#supported-wallets).

#### Selecione um Validador

Depois que uma conta de stake é criada, você provavelmente vai querer delegar o SOL para um nó validador. Abaixo estão alguns lugares onde você pode obter informações sobre os validadores que estão participando atualmente da rede. A equipe Solana Labs e a Fundação Solana não recomendam nenhum validador em particular.

Os principais validadores da Beta introduziram-se e seus serviços neste tópico Solana do Fórum:

- https://forums.solana.com/t/validator-information-thread

O site solanabeach.io é construído e mantido por um de nossos validadores, Instalações Locais. Fornece algumas informações gráficas de alto nível sobre a rede como um todo, assim como uma lista de cada validador e algumas estatísticas de desempenho recentes de sobre cada um.

- https://solanabeach.io

Para ver as estatísticas de produção de blocos, use as ferramentas de linha de comando Solana:

- `validadores solana`
- `produção da solana`

A equipe Solana não faz recomendações sobre como interpretar esta informação de. Os potenciais delegadores devem fazer o seu próprio dever de diligência.

#### Delegar seu Stake

Depois de ter decidido quais validadores ou validadores você irá delegar, use uma carteira suportada para delegar sua conta de participação para o endereço de conta do validador.

## Detalhes conta de Stake

Para mais informações sobre as operações e permissões associadas a uma conta de participação, por favor consulte [Contas de Participação](staking/stake-accounts.md)
