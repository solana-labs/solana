---
title: Rustクライアント
---

## 問題

`クライアント` の形質に関して、ベンチ-tps などの高レベルテストが記述されています。 テストスイートの一部としてこれらのテストを実行する場合、 低レベルの `BankClient` の実装を使用します。 クラスタに対して同じテストを実行する必要がある場合は、 `ThinClient` の実装を使用します。 アプローチの問題はトレートが継続的に拡張され、新しいユーティリティ関数を含めるようになってしまい、そのすべての実装が新しい機能を追加する必要があるということです。 ユーザーフレンドリーなオブジェクトを、ネットワークインターフェースを抽象化したトレートから分離することで、トレートやその実装を拡張することなく、"RpcClient"の "spinner "のようなあらゆる種類の便利な機能を含むようにユーザーフレンドリーなオブジェクトを拡張することができます。

## 提案されたソリューション

`クライアント` トレートを実装する代わりに、 `ThinClient` を 実装して構築する必要があります。 これにより、`クライアント` トレート内のすべてのユーティリティ関数は、 `ThinClient` に移動できます。 `ThinClient` は、`solana-sdk` に移行する可能性があります。なぜなら、すべてのネットワーク依存関係は`クライアント` の実装 であるからです。 そして、`"ClusterClient"`と呼ばれる新しい`"Client"`の実装を追加します。この実装は、現在`"ThinClient"`が存在する`"solana-client" `クレートに格納されます。

この再編成の後、クライアントを必要とするすべてのコードは`ThinClient` の観点から書かれます。 ユニットテストでは、`ThinClient<BankClient>`で機能を呼び出し、`main()`関数やベンチマーク、統合テストでは`ThinClient<ClusterClient>`で機能を呼び出します。

上位のコンポーネントが、`"BankClient"`で実装できる以上の機能を必要とする場合は、ここで説明したのと同じパターンで、第 2 のトレートを実装した第 2 のオブジェクトで実装する必要があります。

### エラー処理

`Client`は、`Custom(String)`フィールドが`Custom(Box<dyn Error>)`に変更されることを除いて、エラーのために既存の`TransportError`列挙を使用する必要があります。

### 実装戦略

1. `solana-sdk`に新しいオブジェクト、`RpcClientTng`を追加しました。`Tng`というサフィックスは一時的なもので、"The Next Generation "の略です。
2. `RpcClientTng` を `SyncClient` 実装で初期化します。
3. `solana-sdk`に新しいオブジェクト、`ThinClientTng`を追加し、`RpcClientTng`と`AsyncClient`の実装で初期化します。
4. `BankClient` から `ThinClientTng<BankClient>` にすべてのユニットテストを移動します。
5. `ClusterClient` を追加します。
6. `ThinClient` ユーザを `ThinClientTng<ClusterClient>に移動`します。
7. `ThinClient` を削除し、 `ThinClientTng` を `ThinClient` に変更します。
8. `RpcClient` のユーザーを新しい `ThinClient<ClusterClient>` に移動します。
9. `RpcClient` を削除し、 `RpcClientTng` を `RpcClient` に変更します。
