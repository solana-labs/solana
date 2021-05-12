---
title: Programas de teste
---

As aplicações enviam transações para um cluster Solana e consultam os validadores para confirmar que as transações foram processadas e para verificar o resultado de cada transação. Quando o cluster não se comporta como antecipado, ele pode ser por várias razões:

- O programa está com problemas
- O carregador BPF rejeitou uma instrução insegura do programa
- A transação foi muito grande
- A transação foi muito grande
- O Runtime tentou executar a transação quando outro estava acessando

  a mesma conta

- A rede desligou a transação
- O agrupamento refez o livro-razão
- Um validador respondeu a consultas maliciosas

## The AsyncClient and SyncClient Traits

Para solucionar problemas, o aplicativo deve redirecionar um componente de nível inferior, onde são possíveis menos erros. Redirecionamento pode ser feito com diferentes implementações dos traços AsyncClient e SyncClient.

Componentes implementam os seguintes métodos primários:

```text
trait AsyncClient {
    fn async_send_transaction(&self, transaction: Transaction) -> io::Result<Signature>;
}

trait SyncClient {
    fn get_signature_status(&self, signature: &Signature) -> Result<Option<transaction::Result<()>>>;
}
```

Os usuários enviam transações e aguardam resultados de forma gradual e sincronizada.

### ThinClient para Clusters

A implementação de nível mais elevado, ThinClient, visa um cluster Solana, que pode ser um testnet implantado ou um cluster local rodando em uma máquina de desenvolvimento.

### TpuClient para o TPU

O próximo nível é a implementação do TPU, que ainda não foi implementada. Ao nível do TPU, o aplicativo envia as transações através dos canais Rust onde não pode haver surpresas com filas de rede ou pacotes soltados. O TPU implementa todos os erros das transações "normais". Ele faz verificação de assinaturas, pode relatar erros em uso de conta e de outra forma resultados no livro, completo com prova de hashes do histórico.

## Teste de nível baixo

### BankClient do Banco

Abaixo do nível de TPU está o banco. O Banco não faz verificação de assinatura nem gera um livro de contas. O Banco é uma camada conveniente para testar novos programas em cadeia. Ele permite que os desenvolvedores alternem entre implementações nativas do programa e variantes compiladas pelo BPF. Não há necessidade da característica Transact aqui. A API do Banco é síncrona.

## Teste Unit com o Runtime

Abaixo do Banco é o tempo de corrida. O Runtime é o ambiente de teste ideal para testes unitários. Ao ligar estaticamente o Runtime a uma implementação nativa do programa, o desenvolvedor ganha o loop de edição mais curto possível. Sem qualquer ligação dinâmica, os rastros incluem símbolos de depuração e erros de programa são diretos para solucionar problemas.
