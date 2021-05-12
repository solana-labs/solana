---
title: Propagação de Bloco Turbina
---

Um cluster Solana usa um mecanismo de propagação de blocos com múltiplas camadas chamado _Turbine_ para transmitir uma transação a todos os nós com quantidade mínima de mensagens duplicadas. O cluster se divide em pequenas coleções de nós, chamadas de _bairros_. Cada nó é responsável por compartilhar quaisquer dados que recebe com os outros nós da sua vizinhança, Além de divulgar os dados a um pequeno conjunto de nós de outros bairros. Desta forma, cada nó só tem que se comunicar com um pequeno número de nós.

Durante seu slot, o nó líder distribui pedaços entre os nós validadores do primeiro bairro \(camada 0\). Cada validador compartilha seus dados dentro de seu bairro, mas também retransmite os destroços para um nó em alguns bairros na próxima camada \(camada 1\). Os nós de camada 1 compartilham seus dados com seus pares de bairro, e retransmitir para os nós da próxima camada, etc, até que todos os nós do cluster tenham recebido todas as divisões.

## Atribuição de Vizinhança - Seleção Ponderada

Para que o avião possa funcionar, todo o aglomerado tem de chegar a acordo sobre a forma como o aglomerado é dividido em bairros. Para conseguir isso, todos os nós de validador reconhecidos \(os pares da TVU\) são classificados por stake e armazenados em uma lista. Esta lista é então indexada de diferentes maneiras para descobrir os limites da vizinhança e retransmitir pares. Por exemplo, o líder simplesmente selecionará os primeiros nós para formar a camada 0. Estes serão automaticamente os titulares mais importantes, permitindo que os votos mais pesados voltem ao líder primeiro. Nódulos de camada 0 e inferior usam a mesma lógica para encontrar seus vizinhos e próximos jogadores de camada.

Para reduzir a possibilidade de vetores de ataque, cada shred é transmitido por uma árvore aleatória de bairros. Cada nó usa o mesmo conjunto de nós que representam o cluster. Uma árvore aleatória é gerada a partir do conjunto para cada rasgo usando uma semente derivada do índice de id da líder, slot e pequeno índice.

## Estrutura de Camada e Vizinhança

O líder atual faz suas transmissões iniciais para no máximo `DATA_PLANE_FANOUT`. Se esta camada 0 é menor que o número de nós no cluster, então o mecanismo de fanout do avião de dados adiciona camadas abaixo. Camadas subsequentes seguem estas restrições para determinar a capacidade de camada: Cada bairro contém `DATA_PLANE_FANOUT` nós. Layer 0 começa com 1 bairro com nós fanout. O número de nós em cada camada adicional aumenta por um factor de fantoche.

Como mencionado acima, cada nó em uma camada só precisa transmitir suas divisas aos seus vizinhos e exatamente 1 nó em alguns bairros da próxima camada, em vez de cada par de TVU no cluster. Uma boa maneira de pensar sobre isto é, a camada 0 começa com 1 bairro com nós de fanfarra, a camada 1 acrescenta bairros, cada um com nós de fanout e camada 2 terá `fanout * número de nós na camada 1` e assim por diante.

Dessa forma, cada nó só tem que se comunicar com um máximo de `2 * DATA_PLANE_FANOUT - 1 nó`.

O diagrama a seguir mostra como o Leader manda os 2 anos de Bairro 0 em Layer 0 e como os nós de Vizinhança 0 compartilham entre si seus dados.

![Líder envia centenas a Vizinhança 0 na Layer 0](/img/data-plane-seeding.svg)

O diagrama a seguir mostra como a Vizinhança 0 varia para os Vizinhos 1 e 2.

![Vizinhança 0 Fanout a Vizinhança 1 e 2](/img/data-plane-fanout.svg)

Por último, o diagrama seguinte mostra um aglomerado de duas camadas com um fanout de 2.

![Duas camadas de agrupamento com um Fanout de 2](/img/data-plane.svg)

### Valores de Configuração

`DATA_PLANE_FANOUT` - Determina o tamanho da camada 0. Camadas subsequentes crescem por um fator de `DATA_PLANE_FANOUT`. O número de nós num bairro é igual ao valor da pompa. Os vizinhos preencherão a capacidade antes que novas capacidades sejam adicionadas, ou seja, se um bairro não estiver cheio, _deve ser_ o último.

Atualmente, a configuração é definida quando o cluster é iniciado. No futuro, esses parâmetros podem ser hospedados na cadeia, permitindo a modificação na mosca conforme os tamanhos de agrupamento mudarem.

## Calculando a taxa FEC necessária

A Turbina depende da retransmissão de pacotes entre validadores. Devido a retransmissão, qualquer perda de pacote grande de rede é composta, e a probabilidade do pacote falhando em alcançar seu destino aumenta em cada salto. A taxa FEC precisa levar em conta a perda de pacotes de rede de, e a profundidade da propagação.

Um grupo shred é o conjunto de pacotes de dados e codificação que podem ser usados para reconstruir um ao outro. Cada grupo tem uma chance de falha, com base na probabilidade do número de pacotes falhados que excedem a taxa FEC. Se um validador não consegue reconstruir o grupo rasgado, então o bloco não pode ser reconstruído, e o validador tem que confiar em em reparo para consertar os blocos.

A probabilidade de falha no grupo pode ser calculada usando a distribuição binomial de. Se a taxa FEC é de `16:4`, então o tamanho do grupo é 20, e pelo menos 4 das divisas devem falhar para que o grupo falhe. Que é igual à soma da probabilidade de 4 ou mais trilhas falharem de 20.

Probabilidade de um bloco ter sucesso na turbina:

- Probabilidade de falha de pacote: `P = 1 - (1 - network_packet_loss_rate)^2`
- Taxa FEC: `├M`
- Número de tentativas: `N = K + M`
- Taxa de falha de grupo sombreada: `S = SUM de i=0 -> M para binomial(prob_failed = P, testes = N, falhas = i)`
- Centros por bloco: `G`
- Taxa de sucesso do bloqueio: `B = (1 - S) ^ (G / N)`
- Distribuição binomial para exatamente `i` resultados com probabilidade de P em N testes é definida como `(N choose i) * P^i * (1 - P)^(N-i)`

Por exemplo:

- A taxa de perda do pacote de rede é de 15%.
- A rede de 50k tps gera 6400 pedaços por segundo.
- A taxa da FEC aumenta o total de divisões por bloco na proporção da FEC.

Taxa FEC: `├M`

- `G = 8000`
- `P = 1 - 0.85 * 0,85 = 1 - 0,7225 = 0,2775`
- `S = SUM de i=0 -> 4 para binomial(prob_failed = 0.2775, testes = 20, falhas = i) = 0.689414`
- `B = (1 - 0.689) ^ (8000 / 20) = 10^-203`

Taxa FEC: `├M`

- `G = 12800`
- `S = SUM de i=0 -> 32 para binomial(prob_failed = 0.2775, testes = 64, falhas = i) = 0.002132`
- `B = (1 - 0.002132) ^ (12800 / 32) = 0.42583`

Taxa FEC: `├M`

- `G = 12800`
- `S = SUM de i=0 -> 32 para binomial(prob_failed = 0.2775, testes = 64, falhas = i) = 0.000048`
- `B = (1 - 0.000048) ^ (12800 / 64) = 0.99045`

## Vizinhos

O diagrama seguinte mostra como dois bairros em diferentes camadas interagem. Para falsificar um bairro, nós suficientes \(códigos de eliminação +1\) do bairro acima precisam falhar. Desde que cada bairro recebe pedaços de vários nós de um bairro na camada superior, Precisamos de uma grande falha de rede nas camadas superiores para acabar com dados incompletos.

![Funcionamento interno de um bairro](/img/data-plane-neighborhood.svg)
