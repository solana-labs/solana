---
title: Transação Durável Nonces
---

## Problema

Para evitar o replay, as transações Solana contêm um campo inexistente preenchido com um valor hash de hash "recente". Uma transação contendo um blockhash muito antigo (~2min a partir desta escrita) é rejeitada pela rede como inválida. Infelizmente certos casos de uso, como serviços de custodismo, requerem mais tempo para produzir uma assinatura para a transação. É necessário um mecanismo para habilitar esses potencialmente participantes da rede offline.

## Requisitos

1. A assinatura da transação precisa cobrir o valor nonce
2. O nonce não deve ser reutilizável, mesmo no caso de assinar a divulgação da chave

## Uma solução baseada em contratos

Aqui descrevemos um problema com base em contratos. em que um cliente pode "ocultar" um valor nonce para uso futuro no campo de `recent_blockhash` de uma transação. Esta abordagem é semelhante à instrução Compare e troca de atômica implementada por algumas ISA de CPU.

Ao usar um bloqueio durável, o cliente deve primeiro consultar seu valor a partir de dados da conta. Agora uma transação é construída da maneira normal, mas com as seguintes exigências adicionais:

1. O valor nonce durável é usado no campo `recent_blockhash`
2. Uma instrução de `AdvanceNonceAccount` é a primeira emitida na transação

### Mecânica do contrato

TODO: svgbob em um gráfico

```text
Start
Create Account
  state = Uninitialized
NonceInstruction
  if state == Uninitialized
    if account.balance < rent_exempt
      error InsufficientFunds
    state = Initialized
  elif state != Initialized
    error BadState
  if sysvar.recent_blockhashes.is_empty()
    error EmptyRecentBlockhashes
  if !sysvar.recent_blockhashes.contains(stored_nonce)
    error NotReady
  stored_hash = sysvar.recent_blockhashes[0]
  success
WithdrawInstruction(to, lamports)
  if state == Uninitialized
    if !signers.contains(owner)
      error MissingRequiredSignatures
  elif state == Initialized
    if !sysvar.recent_blockhashes.contains(stored_nonce)
      error NotReady
    if lamports != account.balance && lamports + rent_exempt > account.balance
      error InsufficientFunds
  account.balance -= lamports
  to.balance += lamports
  success
```

Um cliente que deseja usar esse recurso começa criando uma conta nonce sob o programa de sistema. Esta conta estará no estado `Não inicializado` com nenhum hash armazenado, e portanto inutilizável.

Para inicializar uma conta recém-criada, uma instrução `InitializeNonceAccount` deve ser emitida. Esta instrução recebe um parâmetro, a `Pubkey` da autoridade [da conta](../offline-signing/durable-nonce.md#nonce-authority). Contas Nonce deve estar [isento de aluguel](rent.md#two-tiered-rent-regime) para atender os requisitos de persistência de dados do recurso, e como tal, exige que lâmpadas suficientes sejam depositadas antes de serem inicializadas. Após a inicialização bem sucedida, o blockhash mais recente do cluster é armazenado juntamente com a autoridade nonce especificada `Chave de publicação`.

A instrução</code> da `AdvanceNonceAccount é usada para gerenciar o valor
armazenado nonce da conta. Armazena o blockhash mais recente do cluster nos dados de estado da conta,
falhando se isso corresponder ao valor já armazenado. Esta verificação impede que
reexiba transações dentro do mesmo bloco.</p>

<p spaces-before="0">Devido à exigência de <a href="rent.md#two-tiered-rent-regime">isenção de aluguel</a> </a> isenção de nonce das contas,
uma instrução de retirada personalizada é usada para mover fundos para fora da conta.
A instrução <code>WithdrawNonceAccount` leva um único argumento, lâmpadas para retirar, e impõe isenção de aluguel, evitando que o saldo da conta caia abaixo do mínimo isento de aluguel. Uma exceção para essa verificação é se o saldo final seria zero lâmpadas, o que torna a conta elegível para exclusão. Este detalhe de fechamento de conta tem uma exigência adicional de que o valor nonce armazenado não deve corresponder ao blockhash mais recente do cluster, como por `AdvanceNonceAccount`.

A [autoridade da conta](../offline-signing/durable-nonce.md#nonce-authority)nonce da pode ser alterada usando a `instrução AuthorizeNonceAccount`. Toma um parâmetro, a `Chave de publicação` da nova autoridade. Executar esta instrução concede controle total sobre a conta e seu saldo para a nova autoridade.

> `AdvanceNonceAccount`, `WithdrawNonceAccount` e `AuthorizeNonceAccount` requerem a atual [authority](../offline-signing/durable-nonce.md#nonce-authority) para que a conta assine a transação.

### Suporte a Runtime

O contrato por si só não é suficiente para implementar esse recurso. Para impor um `recent_blockhash` na transação e evitar o roubo da taxa via replay de transação falhado, são necessárias modificações em tempo de execução.

Qualquer transação falhando a validação `check_hash_age` habitual será testada para uma Transação Durável Nonce. Isso é sinalizado, incluindo uma instrução `AdvanceNonceAccount` como a primeira instrução na transação.

Se o tempo de execução determinar que uma Transação Durável Nonce está em uso, tomará as seguintes ações adicionais para validar a transação:

1. A `Não Conta` especificada na instrução `Nonce` é carregada.
2. O `Não chega` é deserializado do campo de dados `Não Conta`e confirmado para estar no `estado` inicializado.
3. O valor nonce armazenado no `NonceAccount` é testado para corresponder ao valor especificado no campo `recent_blockhash` da transação.

Se todas as três verificações acima forem bem-sucedidas, a transação poderá continuar validação.

Uma vez que transações que falham com um `InstructionError` são cobradas uma taxa e alterações em seu estado voltou, há uma oportunidade para roubo de taxas se uma instrução `AdvanceNonceAccount` for revertida. Um validador malicioso pode repetir a transação fracassada até que o nonce armazenado seja avançado com sucesso. Runtime muda a prevenção deste comportamento. Quando uma transação durável do nonce falha com `instruçãoErro` além da `AdvanceNonceAccount` instrução, a conta de nonce é reenviada para seu estado de pré-execução, como de costume. Então o tempo de execução avança seu valor nonce e a conta avançada nonce armazenada como se tivesse sucesso.
