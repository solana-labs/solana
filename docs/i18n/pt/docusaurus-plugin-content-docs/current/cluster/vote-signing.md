---
title: Assinatura de Voto Seguro
---

Um validador recebe entradas do líder atual e envia votos confirmando que essas entradas são válidas. Esta apresentação de voto apresenta um desafio de segurança, porque votos forjados que violam as regras de consenso podem ser usados para reduzir a aposta do validador.

O validador vota no fork escolhido ao enviar uma transação que usa uma chave assimétrica para assinar o resultado de seu trabalho de validação. Outras entidades podem verificar esta assinatura usando a chave pública do validador. Se a chave do validador for utilizada para assinar dados incorretos \(e.g. votos em múltiplas bifurcações do livro\), a estaca do nó ou seus recursos poderiam ser comprometidos.

Solana aborda esse risco dividindo um _assinante de voto separado_ que avalia cada voto para garantir que não viole uma condição de corte.

## Validadores, Assinantes de Votar e Participantes

Quando um validador recebe vários blocos para o mesmo slot, ele rastreia todos os forks possíveis até que ele possa determinar um "melhor". O validador seleciona o melhor fork ao submeter um voto a ele, usar um assinante de voto para minimizar a possibilidade de seu voto violar inadvertidamente uma regra de consenso e conseguir uma participação reduzida.

Um assinante de voto avalia a votação proposta pelo validador e assina a votação apenas se não violar uma condição decrescente. Um assinante de voto só precisa manter um estado mínimo em relação aos votos que assinou e aos votos assinados pelo resto do cluster. Não precisa processar um conjunto completo de transações.

Uma parte interessada é uma identidade que controla o capital em causa. As partes interessadas podem delegar a sua participação no eleitorado. Uma vez delegada uma parte, os votos dos signatários representam o peso dos votos de todas as apostas delegadas, e produza recompensas para todas as apostas delegadas.

Atualmente, existe uma relação de 1:1 entre validadores e assinantes do voto, e as partes interessadas delegam toda sua participação a um único assinante de voto.

## Serviço de assinatura

O serviço de assinatura de votos consiste em um servidor RPC JSON e um processador de pedidos. Na inicialização, o serviço inicia o servidor RPC em uma porta configurada e espera por solicitações de validador. Ele espera o seguinte tipo de pedidos:

1. Registra um novo nó de validador

   - The request must contain validator's identity \(public key\)
   - The request must be signed with the validator's private key
   - The service drops the request if signature of the request cannot be verified
   - The service creates a new voting asymmetric key for the validator, and returns the public key as a response
   - If a validator tries to register again, the service returns the public key from the pre-existing keypair

2. Sign a vote

   - The request must contain a voting transaction and all verification data
   - The request must be signed with the validator's private key
   - The service drops the request if signature of the request cannot be verified
   - The service verifies the voting data
   - The service returns a signature for the transaction

## Validator voting

A validator node, at startup, creates a new vote account and registers it with the cluster by submitting a new "vote register" transaction. The other nodes on the cluster process this transaction and include the new validator in the active set. Subsequently, the validator submits a "new vote" transaction signed with the validator's voting private key on each voting event.

### Configuration

The validator node is configured with the signing service's network endpoint \(IP/Port\).

### Registration

At startup, the validator registers itself with its signing service using JSON RPC. The RPC call returns the voting public key for the validator node. The validator creates a new "vote register" transaction including this public key, and submits it to the cluster.

### Vote Collection

The validator looks up the votes submitted by all the nodes in the cluster for the last voting period. This information is submitted to the signing service with a new vote signing request.

### New Vote Signing

The validator creates a "new vote" transaction and sends it to the signing service using JSON RPC. The RPC request also includes the vote verification data. On success, the RPC call returns the signature for the vote. On failure, RPC call returns the failure code.
