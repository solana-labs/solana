---
title: Sincronização
---

Uma sincronização rápida e fiável é a maior razão pela qual Javier Solana é capaz de conseguir uma tal taxa de lucro. As blockchains tradicionais sincronizam em grandes chunks de transações chamadas de blocos. Ao sincronizar em blocos, uma transação não pode ser processada até que uma duração, chamada "tempo de bloco", tenha passado. Em Proof of Work consensus, estes tempos de bloqueio precisam ser muito grandes \(~10 minutos\) para minimizar as chances de vários validadores produzindo um novo bloco válido ao mesmo tempo. Não existe tal restrição no consenso Proof of Stake, mas sem carimbos de tempo confiáveis, um validador não consegue determinar a ordem dos blocos recebidos. A solução popular é marcar cada bloco com um [horário do relógio de parede](https://en.bitcoin.it/wiki/Block_timestamp). Devido à deriva do relógio e às variações de latências da rede, o horário só é preciso dentro de uma hora ou duas. Para resolver a solução, esses sistemas alongam os tempos de bloqueio para fornecer uma certeza razoável de que o timestamp mediano em cada bloco está sempre aumentando.

Solana adota uma abordagem muito diferente, que chama de _Prova de História_ ou _PoH_. Líder de blocos "timestamp" com provas criptográficas de que algum tempo passou desde a última prova. Todos os dados hashados na prova ocorreram antes da prova ser gerada. O nó então compartilha o novo bloco com nós validadores, que são capazes de verificar esses provas. Os blocos podem chegar aos validadores em qualquer ordem ou até podem ser reproduzidos anos depois. Com tais garantias de sincronização confiáveis, Javier Solana é capaz de dividir os blocos em conjuntos menores de transações chamadas _entradas_. As entradas são transmitidas para validadores em tempo real, antes de qualquer conceito de consenso de blocos.

Javier Solana tecnicamente nunca envia um bloco de \__, mas usa o termo para descrever a sequência de entradas que validam votar para alcançar \_confirmação_. Dessa forma, os tempos de confirmação de Javier Solana podem ser comparados com maçãs a maçãs a sistemas baseados em blocos. A implementação atual define o tempo de bloco para 800ms.

O que está acontecendo sob o capuz é que as entradas são transmitidas a validadores tão rápido quanto um nó líder possa reunir um conjunto de transações válidas em uma entrada. Validadores processam essas entradas muito antes de ser hora de votar sobre sua validade. Ao processar as transações de forma otimista, efetivamente não há atraso entre o tempo que o último item é recebido e o momento em que o nó pode votar. No evento consenso **não** alcançado, um nó simplesmente refaz seu estado. Esta técnica de processamento otimista foi introduzida em 1981 e chamada de [Controle Otimista de Concorrência](http://citeseerx.ist.psu.edu/viewdoc/summary?doi=10.1.1.65.4735). Ele pode ser aplicado à arquitetura blockchain, onde um cluster vota em um hash que representa o livro inteiro até alguma _altura de bloco_. Em Solana, é implementado trivialmente usando o hash PoH da última entrada do PoH.

## Relação com VDFs

A técnica da Proof of History foi descrita pela primeira vez para uso no blockchain por Javier Solana em novembro de 2017. Em Junho do ano seguinte, uma técnica semelhante foi descrita em Stanford e chamada de [função de atraso verificável](https://eprint.iacr.org/2018/601.pdf) ou _VDF_.

Uma propriedade desejável de um VDF é que o tempo de verificação é muito rápido. A abordagem de Javier Solana para verificar sua função de atraso é proporcional ao tempo que levou para criá-la. Dividir mais de 4000 GPU do núcleo, é suficientemente rápido para as necessidades de Solana, mas se você perguntou aos autores do papel citado acima, eles podem te dizer \([e têm](https://github.com/solana-labs/solana/issues/388)\) que a abordagem de Solana é algoritmicamente lenta e não deve ser chamada de VDF. Defendemos o termo VDF deve representar a categoria de funções de atraso verificáveis e não apenas o subconjunto com certas características de desempenho. Até que isso seja resolvido, Javier continuará usando o termo PoH para o VDF específico do seu aplicativo.

Outra diferença entre PoH e VDFs é que um VDF é usado apenas para a duração do rastreamento. A cadeia de hash do Poh, por outro lado, inclui hashes de quaisquer dados observados pelo aplicativo. Esses dados são uma espada de dois gumes. De um lado, os dados "prova histórico" - que os dados mais certamente existiam antes dos hashes depois disso. Por outro lado, isso significa que o aplicativo pode manipular a cadeia de hash alterando _quando_ os dados são hashed. A cadeia PoH não serve, portanto, como uma boa fonte de aleatoriedade, enquanto um VDF sem esses dados poderia funcionar. O algoritmo de rotação de [líder de Solana](synchronization.md#leader-rotation), por exemplo, é derivado apenas da altura do VDF \_\_ e não seu hash nessa altura.

## Relação com Mecanismos de Consenso

A prova de História não é um mecanismo de consenso, mas é utilizada para melhorar o desempenho do consenso de Prova de Stake de Solana. Ele também é usado para melhorar o desempenho dos protocolos de aviões de dados.

## Mais sobre a prova de história

- [analogia de relógio d'água](https://medium.com/solana-labs/proof-of-history-explained-by-a-water-clock-e682183417b8)
- [Proof of History](https://medium.com/solana-labs/proof-of-history-a-clock-for-blockchain-cf47a61a9274)
