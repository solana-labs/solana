---
title: "개요"
---

[앱](terminology.md#app)은 다수의 [명령](transactions.md#instructions)을 담은 [트랜잭션](transactions.md)을 솔라나 클러스터로 보내면서 상호작용 합니다. 솔라나 [런타임](runtime.md)은 이 명령들을 이전에 개발자들이 배포한 [프로그램](terminology.md#program)으로 넘깁니다. 명령은 프로그램에게 [램포트](terminology.md#lamports)를 한 [계정](accounts.md)에서 다른 계정으로 옮기라는 명령이거나, 램포트가 어떻게 전송되는 지 관리하는 상호작용성 컨트랙트를 생성하는 것일 수도 있습니다. 명령들은 순차적으로, 그리고 각 트랜잭션 당 개별적으로 처리됩니다. 단일 명령만이 부적합하더라도, 트랜잭션 상 계정의 변화는 모두 폐기됩니다.

바로 개발을 시작하려면 다음 [예시](developing/deployed-programs/examples.md)들로 개발하고, 배포하고, 실행해 보세요.