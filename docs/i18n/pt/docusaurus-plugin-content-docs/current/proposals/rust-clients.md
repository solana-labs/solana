---
title: Limpar Clientes
---

## Problema

Testes de alto nível, como bench-tps, são escritos em termos do traço `do cliente`. Quando executamos esses testes como parte do conjunto de testes, usamos a implementação do BankClient `de baixo nível`. Quando precisamos executar o mesmo teste contra um cluster, usamos a implementação `ThinClient`. O problema com essa abordagem é que isso significa que a característica será continuamente expandida para incluir novas funções de utilitário e todas as implementações dele precisam para adicionar a nova funcionalidade. Separando o objeto do usuário virado da característica que abstrai a interface da rede, podemos expandir o objeto do usuário para incluir todos os tipos de funcionalidades úteis, como o "spinner" do RpcClient, sem preocupação por precisar estender a característica e suas implementações.

## Solução Proposta

Em vez de implementar a característica</code> do `cliente, <code>ThinClient` deveria ser construído com uma implementação dele. Dessa forma, todas as funções de utilitário atualmente na característica `cliente` pode mudar para `ThinClient`. `ThinClient` poderia então ir para `solana-sdk` já que todas as suas dependências de rede estariam na implementação do `cliente`. Adicionaríamos uma nova implementação de `Cliente`, chamada `ClusterClient`, e isso viveria na caixa de `solana-client`, onde `ThinClient` encontra-se no momento.

Após esta reorganização, qualquer código que precise de um cliente será escrito em termos de `ThinClient`. Em testes unitários, a funcionalidade seria chamada com `ThinClient<BankClient>`, enquanto `main()` funções, testes de referência e integração iriam invocá-lo com `ThinClient<ClusterClient>`.

Se componentes de nível superior requerem mais funcionalidades do que o que poderia ser implementado pelo `BankClient`,, ele deve ser implementado por um segundo objeto que implemente uma segunda característica, seguindo o mesmo padrão descrito aqui.

### Tratamento de erros

O cliente `` deve usar o existente `TransportError` enum para erros, exceto que o campo `personalizado (String)` deve ser alterado para `Custom(Box<dyn Error>`.

### Estratégia de implementação

1. Adicionar novo objeto para `solana-sdk`, `RpcClientTng`, onde o sufixo `Tng` é temporário e significa "A próxima geração"
2. Inicialize `RpcClientTng` com uma implementação `SyncClient`.
3. Adicionar novo objeto ao `solana-sdk`, `ThinClientTng`; inicializá-lo com `RpcClientTng` e uma implementação `AsyncClient`
4. Mover todos os testes unitários do `BankClient` para `ThinClientTng<BankClient>`
5. Adicionar `ClusterClient`
6. Mover `usuários do ThinClient` para `ThinClientTng<ClusterClient>`
7. Excluir `ThinClient` e renomear `ThinClientTng` para `ThinClient`
8. Mover `usuários do ThinClient` para `ThinClientTng<ClusterClient>`
9. Excluir `ThinClient` e renomear `ThinClientTng` para `ThinClient`
