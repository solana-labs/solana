---
title: Alugue
---

As contas de Solana podem ter estado controlado pelo proprietário \(`Conta::data`\) que é separado do saldo da conta \(`Account::lamports`\). Desde que os validadores na rede precisam manter uma cópia de funcionamento deste estado na memória, a rede cobra uma taxa baseada no tempo e no espaço para este consumo de recursos, também conhecida como aluguel.

## regime de aluguel de duas faixas

As contas que mantêm um saldo mínimo equivalente a 2 anos de pagamento de rendas estão isentas. Os _2 anos_ são desenhados do custo do hardware cai 50% no preço a cada 2 anos e da convergência resultante devido ao fato de ser uma série geométrica. Contas cujo saldo cai abaixo deste limite são cobradas aluguéis a uma taxa especificada em genesis, em lâmpadas por byte-ano. A rede cobra o aluguel por período, em crédito para o próximo período e `Account::rent_epoch` mantém controle da próxima vez que o aluguel deve ser coletado da conta.

Atualmente, o custo da renda é fixo no génesis. No entanto, prevê-se que seja dinâmico, reflectindo o custo do armazenamento de hardware subjacente no momento. Portanto, espera-se que o preço diminua à medida que o custo do hardware diminui à medida que a tecnologia avança.

## Tempo para coletar aluguer

Há dois momentos para coletar aluguel das contas: \(1\) quando referenciado por uma transação, \(2\) periodicamente uma vez por período. \(1\) inclui a transação para criar a nova conta em si, e acontece durante o processamento normal de transações pelo banco como parte da fase de carga. \(2\) existe para garantir a coleta de aluguéis de contas obsoletas, que não são referenciados em épocas recentes. \(2\) requer todo o escaneamento de contas e é distribuído por uma época, com base no prefixo do endereço da conta para evitar picos de carga devido a esta coleção de aluguéis.

Pelo contrário, a coleção de aluguel não é aplicada a contas que são diretamente manipuladas por qualquer processo de contabilidade de nível do protocolo incluindo:

- A distribuição da coleta de aluguel (Caso contrário, pode causar tratamento recursivo da coleta)
- A distribuição das recompensas de staking no início de cada período (Para reduzir tanto quanto o processamento no início de um novo período de tempo)
- A distribuição da taxa de transação no final de cada slot

Mesmo que esses processos estejam fora do escopo da coleta de aluguel, todos as contas manipuladas acabarão sendo tratadas pelo mecanismo \(2\).

## Processamento real da coleta de aluguel

Aluguel é devido por um período de tempo, e as contas têm `Conta::rent_epoch` de `current_epoch` ou `current_epoch + 1` dependendo do regime de aluguel.

Se a conta estiver no regime de isenção, `Account::rent_epoch` é simplesmente atualizado para `current_epoch`.

Se a conta não estiver isenta, a diferença entre o próximo período e `Account::rent_epoch` é usada para calcular a quantidade de aluguel devida por esta conta \(via `Rent::due()`\). Quaisquer lâmpadas fracionárias do cálculo são truncadas. Aluguel due deduzido de `Account::lamports` and `Account::rent_epoch` is updated to `current_epoch + 1` (= next epoch). Se a quantidade de aluguel devida for menor que um lâmpado, nenhuma alteração será feita na conta.

As contas cujo saldo é insuficiente para satisfazer as rendas que se devem simplesmente não carregar.

Uma percentagem do aluguel recolhido é destruída. O resto é distribuído para contas validadoras pelo peso de estacao, taxas de transação no final de cada slot.

Finalmente, a coleção de aluguel acontece de acordo com as atualizações da conta de protocolo, como a distribuição de aluguel aos validadores, o que significa que não há transação correspondente para deduções de aluguel. Portanto, a coleção de aluguel é bastante invisível, apenas implicitamente observável por uma transação recente ou temporização predeterminado dado o prefixo de endereço da conta de sua conta.

## Considerações de design

### Razão do design atual

Sob o modelo anterior, não é possível ter contas que continuam a ser tocadas, que nunca têm de pagar o aluguel. As contas sempre pagam o aluguel exatamente uma vez para cada epoch, exceto as contas de aluguel, sysvar e executável.

Esta escolha é do design pretendido. Caso contrário, seria possível acionar uma coleção de aluguel não autorizado com a instrução `Noop` por qualquer um que possa lucrar injustamente o aluguel (um líder no momento) ou salvar o custo de aluguel previsto para o aluguel.

Como outro efeito colateral desta escolha, também note que esta coleção de renda periódica força o validador a não armazenar contas obsoletas em um armazenamento otimista e salvar o custo de armazenamento, que não é favorável para os proprietários de contas e pode fazer com que transações neles fiquem mais tempo do que as outras. Do lado da barra, isto impede que usuários maliciosos acumulem uma quantidade significativa de contas de lixo, sobrecarregando validadores.

Como consequência geral deste design, todas as contas são armazenadas igualmente como conjunto de trabalho do validador com características de mesmo desempenho, Reflectindo diretamente a estrutura de precificação de aluguel uniforme.

### Coleção Ad-hoc

Foi considerado o aluguel sempre que as contas foram carregadas/acessadas\). As questões com essa abordagem são:

- contas carregadas como "apenas crédito" para uma transação poderiam muito razoavelmente ser esperadas para ter aluguel devido,

  mas não seria gravável durante qualquer transação

- um mecanismo para "bater nos ônibus" \i.e. vá encontrar contas que precisam pagar o aluguel\) é desejável,

  contas que são carregadas com pouca frequência recebem um passeio gratuito

### Instrução do sistema para coletar aluguel

Coletar aluguel por meio de instrução do sistema foi considerado, uma vez que, naturalmente, teria distribuído a renda a nós activos e ponderados e poderia ter sido feito incrementalmente. (ExplicitText="ver:

- teria influenciado negativamente a transferência da rede
- exigiria um caso especial pelo tempo de execução, já que as contas com proprietários que não são do SystemProgram podem ser debitadas por esta instrução
- alguém teria que emitir as transações
