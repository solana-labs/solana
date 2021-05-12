---
title: O Runtime
---

## O Runtime

O tempo de execução é um processador de transação simultâneo. As transações especificam suas dependências de dados na frente e a alocação dinâmica de memória é explícita. Ao separar o código do programa do estado em que ele opera, o tempo de execução é capaz de coreografia de acesso simultâneo. Transações que acessam contas somente de leitura são executadas em paralelo enquanto as transações que acessam contas graváveis são serializadas. O tempo de execução interage com o programa através de um ponto de entrada com uma interface bem definida. Os dados armazenados em uma conta são um tipo opaco, uma matriz de bytes. O programa tem controle total sobre seu conteúdo.

A estrutura de transação especifica uma lista de chaves públicas e assinaturas para essas chaves e uma lista sequencial de instruções que irão operar sobre os estados associados às chaves da conta. Para que a transação seja confirmada, todas as instruções devem ser executadas com sucesso; se qualquer uma abortar a transação inteira falhar em enviar.

#### Estrutura da conta

As contas mantêm um saldo de lâmpadas e memória específica dos programas.

## Motor de transação

O motor mapeia chaves públicas para as contas e as roteia para o ponto de entrada do programa.

### Execução

As transações são processadas em lote e processadas em pipeline. A TPU e a TVU seguem um caminho ligeiramente diferente. O tempo de execução do TPU garante que o registro PoH ocorra antes que a memória seja confirmada.

O TVU tempo de execução garante que a verificação do PoH ocorra antes que o tempo de execução processe quaisquer transações.

![Processamento de runtime](/img/runtime.svg)

No estágio _executa_, as contas carregadas não possuem dependências de dados, então todos os programas podem ser executados em paralelo.

O tempo de execução impõe as seguintes regras:

1. Somente o programa _proprietário_ pode modificar o conteúdo de uma conta. Isso significa que o vetor de dados na atribuição é garantido que será zero.
2. O saldo total em todas as contas é igual antes e após a execução de uma transação.
3. Depois que a transação é executada, os saldos de contas somente leitura devem ser iguais aos saldos antes da transação.
4. Todas as instruções da transação foram executadas atomicamente. Se falhar, todas as modificações da conta serão descartadas.

A execução do programa envolve mapear a chave pública do programa para um ponto de entrada que leva um ponteiro para a transação, e um array de contas carregadas.

### Interface do Sistema

A interface é melhor descrita pela `instrução::data` que o usuário codifica.

- `CriarConta` - Isso permite ao usuário criar uma conta com uma matriz de dados alocada e atribuí-la a um programa.
- `CreateAccountWithSeed` - Mesmo que `CreateAccount`, mas o endereço da nova conta é derivado de
  - a conta de financiamento publicada,
  - uma semente mnemônica (semeada), e
  - a publicação do Programa
- `Atribuir` - Permite que o usuário atribua uma conta existente a um programa.
- `Transferir` - Transferir lâmpadas entre contas.

### Estado do programa de segurança

Para que a blockchain funcione corretamente, o código do programa deve ser resiliente às entradas de usuários. É por isso que neste design o código específico do programa é o único código que pode alterar o estado da matriz de bytes de dados nas Contas que lhe são atribuídas. É também por isso que `Atribuir` ou `CreateAccount` deve zerar os dados. Caso contrário, não haveria nenhuma maneira possível para o programa distinguir os dados da conta recentemente atribuída de uma transição nativa gerada sem alguns metadados adicionais do tempo de execução para indicar que essa memória é atribuída em vez de gerada nativamente.

Para passar mensagens entre programas, o programa de recebimento deve aceitar a mensagem e copiar o estado. Mas, na prática, uma cópia não é necessária e é indesejável. O programa de recebimento pode ler o estado de outras Contas sem copiá-las, e durante a leitura tem uma garantia do estado do programa de remetente.

### Observações

- Não há alocação dinâmica de memória. O cliente precisa usar instruções `CreateAccount` para criar memória antes de passá-la para outro programa. Essa instrução pode ser composta em uma única transação com a chamada para o programa.
- `CreateAccount` e `Atribuir` garantias de que quando a conta for atribuída ao programa, os dados da Conta são inicializados zero.
- As transações que atribuem uma conta a um programa ou alocar espaço devem ser assinadas pela chave privada do endereço da conta, a menos que a Conta esteja sendo criada por `CreateAccountWithSeed`, nesse caso não há nenhuma chave privada correspondente para o endereço/pubkey da conta.
- Uma vez atribuída ao programa, uma Conta não pode ser reatribuída.
- Runtime garante que o código de um programa é o único código que pode modificar os dados da conta atribuídos à conta.
- O tempo de execução garante que o programa só pode gastar lâmpadas que estão em contas que lhe forem atribuídas.
- O tempo de execução garante que os saldos pertencentes às contas estejam equilibrados antes e após a transação.
- O tempo de execução garante que as instruções sejam executadas com sucesso quando uma transação é realizada.

## Trabalho futuro

- [Continuações e Sinais para transações de longo prazo](https://github.com/solana-labs/solana/issues/1485)
