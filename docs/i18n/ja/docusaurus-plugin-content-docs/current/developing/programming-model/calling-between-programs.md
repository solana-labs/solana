---
title: プログラム間で呼び出す方法
---

## プログラム間の招待

Solana ランタイムでは、クロスプログラムインボケーションと呼ばれる仕組みで、プログラム同士の呼び出しが可能です。 プログラム間の呼び出しは、一方のプログラムが他方のプログラムの命令を呼び出すことで実現します。 呼び出されたプログラムは、呼び出されたプログラムが命令の処理を終えるまで停止します。

例えば、クライアントが 2 つのアカウントを変更するトランザクションを作成し、それぞれが別々のチェーン上のプログラムによって所有されているとします。

```rust,ignore
let message = Message::new(vec![
    token_instruction::pay(&alice_pubkey),
    acme_instruction::launch_mys(&bob_pubkey),
]);
client.send_and_confirm_message(&[&alice_keypair, &bob_keypair], &message);
```

クライアントは、代わりに`acme`プログラムがクライアントに代わって便利に`トークン`命令を呼び出すことを許可することができます。

```rust,ignore
let message = Message::new(vec![
    acme_instruction::pay_and_launch_missiles(&alice_pubkey, &bob_pubkey),
]);
client.send_and_confirm_message(&[&alice_keypair, &bob_keypair], &message);
```

2 つのオンチェーンプログラム`"token"`と`"acme"`があり、それぞれが`"pay()"`と`"launch_missiles()"`という命令を実装しているとすると、"acme"は、`"token"`モジュールで定義されている関数を"クロスプログラムインヴォケーション"で呼び出すことで実装できます。

```rust,ignore
mod acme {
    use token_instruction;

    fn launch_missiles(accounts: &[AccountInfo]) -> Result<()> {
        ...
    }

    fn pay_and_launch_missiles(accounts: &[AccountInfo]) -> Result<()> {
        let alice_pubkey = accounts[1].key;
        let instruction = token_instruction::pay(&alice_pubkey);
        invoke(&instruction, accounts)?;

        launch_missiles(accounts)?;
    }
```

`"invoke()"`は Solana のランタイムに組み込まれており、命令の`"program_id"`フィールドを介して、与えられた命令を`トークン`プログラムにルーティングする役割を果たします。

`invoke`は、呼び出し側が、呼び出される命令が必要とするすべてのアカウントを渡すことを要求することに注意してください。 つまり、"実行可能なアカウント" (命令のプログラム Id と一致する のもの) と、 "命令プロセス"に渡されるアカウントの両方があるということです。

`pay()`を呼び出す前に、ランタイムは、`acme`が`token`が所有するアカウントを変更していないことを確認しなければなりません。 これは、`acme`が`invoke`を呼び出したときのアカウントの現在の状態と、`acme`の命令開始時のアカウントの初期状態とに、ランタイムのポリシーを適用することによって行われます。 `pay()`が完了した後、ランタイムは`token`が`acme`の所有するアカウントを変更していないことを再度確認しなければなりません。ランタイムのポリシーを再度適用しますが、今回は`token`のプログラム ID を使用します。　 最後に、`pay_and_launch_missiles()`が完了した後、ランタイムはランタイムポリシーをもう一度適用しなければなりませんが、通常の場合と同様に、更新されたすべての`pre_*`変数を使用します。 これは通常` pay()`までの`pay_and_launch_missiles()`の実行で無効なアカウントの変更がなく、`pay()`でも無効な変更がなく、`pay()`から`pay_and_launch_missiles()`が戻るまでの実行でも無効な変更がなかった場合、ランタイムは`pay_and_launch_missiles()`全体で無効なアカウントの変更がなかったと経時的に仮定することができ、したがってこれらのアカウント変更をすべてコミットすることができます。

### 特権が必要な説明

ランタイムは、呼び出し側のプログラムに与えられた特権を使用して、呼び出し側にどのような特権を拡張できるかを決定します。 ここでいう特権とは、署名者や書き込み可能なアカウントを指します。 例えば、呼び出し元が処理している命令に署名者や書き込み可能なアカウントが含まれている場合、呼び出し元はその署名者や書き込み可能なアカウントを含む命令を呼び出すことができます。

この特権拡張は、プログラムが不変であるという事実に依存しています。 `acme`プログラムの場合、ランタイムはトランザクションの署名を`トークン`命令の署名として安全に扱うことができます。 ランタイムは、`トークン`命令が`alice_pubkey`を参照していることを確認すると、`acme`命令のキーを調べて、そのキーが署名されたアカウントに対応しているかどうかを確認します。 この場合、そのキーはアリスのアカウントに対応しており、それによって`トークン`プログラムがアリスのアカウントを変更することを許可します。

### プログラム署名済みアカウント

プログラムは、[プログラム由来のアドレス](#program-derived-addresses)を使用して、元のトランザクションでは署名されていない署名済みのアカウントを含む命令を発行することができます。

プログラム由来のアドレスを用いてアカウントに署名するために、プログラムは`invoke_signed()`を呼び出すことができます。

```rust,ignore
        invoke_signed(
            &instruction,
            accounts,
            &[&["First addresses seed"],
              &["Second addresses first seed", "Second address second seed"]],
)?;
```

### コールの深さ

プログラム間の呼び出しにより、プログラムは直接他のプログラムを呼び出すことができますが、 の深さは現在"4"に制限されています。

### Reentrancy

現在、リエントランシーは、一定の深さに制限された直接の自己再帰に限られています。 この制限により、あるプログラムが中間状態から他のプログラムを呼び出す際に、後で呼び戻される可能性があることを知らずに呼び出すような状況を防ぐことができます。 直接再帰では、プログラムは、呼び戻された時点での状態を完全に制御することができます。

## プログラム派生アドレス

プログラム派生アドレスは、[プログラム間の呼び出し](#cross-program-invocations)時に、プログラムが生成した署名を使用することができます。

プログラム由来のアドレスを使用すると、あるプログラムにアカウントの権限を与え、後にその権限を別のプログラムに移すことができます。 これが可能なのは、権限を与える取引において、プログラムが署名者の役割を果たすことができるからです。

例えば、2 人のユーザーが Solana でゲームの結果に賭けをしたい場合、それぞれが賭け金の資産を、合意を尊重してくれる何らかの仲介者に移さなければなりません。 現在、この仲介者を Solana のプログラムとして実装する方法はありません。なぜなら、仲介者のプログラムは勝者に資産を譲渡することができないからです。

多くの DeFi アプリケーションでは、新しい所有者を決定する何らかのイベントが発生するまで、資産をエスクローエージェントに譲渡する必要があるため、この機能が必要となります。

- 買い注文と売り注文をマッチングさせて資産を移転する分散型取引所。

- 落札者に資産を譲渡するオークション

- 景品を集めて勝者に再分配するゲームや予測市場。

プログラム派生アドレス:

1. プログラムは、プログラム・アドレスと呼ばれる特定のアドレスを、外部のユーザがそのアドレスに対する署名を用いて有効なトランザクションを生成できないように制御することができます。

2. [クロスプログラムインヴォケーション](#cross-program-invocations)で呼び出される命令の中に存在するプログラムアドレスに対して、プログラムがプログラム的に署名できるようにします。

この 2 つの条件を満たせば、ユーザーはチェーン上の資産の権限をプログラムのアドレスに安全に移転・譲渡することができ、プログラムはその権限を自分の判断で別の場所に割り当てることができます。

### プログラムアドレスのプライベートキー

プログラムアドレスは"ed25519"曲線上に存在しないため、有効な秘密キーを持たず、そのために署名を生成することは不可能です。 プログラムアドレスは、それ自体は秘密キーを持たないものの、プログラムがプログラムアドレスを署名者として含む命令を発行する際に使用することができます。

### ハッシュベースの生成されたプログラムアドレス

プログラムアドレスは、256 ビットの耐残像性ハッシュ関数を用いて、シードの集合とプログラム ID から決定論的に導き出されます。 プログラムアドレスは，関連する秘密キーが存在しないことを保証するため，"ed25519 曲線上"にあってはなりません。 与えられ生成中にアドレスが曲線上にあることが判明した場合は、エラーが返されます。 この現象が起こる確率は、ある種のシードとプログラム ID の組み合わせでは、およそ半々です。 このような場合には、別のシードセットやシードバンプ(追加の 8 ビットシード) を使用して、有効なプログラムアドレスを曲線の外に見つけることができます。

プログラム用の決定論的プログラムアドレスは、`SystemInstruction::CreateAccountWithSeed` で作成された Account と同様の派生経路をたどりますが、これは `system_instruction::create_address_with_seed`で実装されています。

参考までにその実装は以下の通りです。

```rust,ignore
pub fn create_address_with_seed(
    base: &Pubkey,
    seed: &str,
    program_id: &Pubkey,
) -> Result<Pubkey, SystemError> {
    if seed.len() > MAX_ADDRESS_SEED_LEN {
        return Err(SystemError::MaxSeedLengthExceeded);
    }

    Ok(Pubkey::new(
        hashv(&[base.as_ref(), seed.as_ref(), program_id.as_ref()]).as_ref(),
    ))
}
```

プログラムは、シードを用いることで、任意の数のアドレスを決定論的に導き出すことができます。 この種は、アドレスがどのように使用されるかを象徴的に示すことができます。

`Pubkey`::

```rust,ignore
/// Generate a derived program address
///     * seeds, symbolic keywords used to derive the key
///     * program_id, program that the address is derived for
pub fn create_program_address(
    seeds: &[&[u8]],
    program_id: &Pubkey,
) -> Result<Pubkey, PubkeyError>
```

### プログラムアドレスの使用

クライアントは `create_program_address` 関数を使用して宛先 アドレスを生成できます。

```rust,ignore
// deterministically derive the escrow key
let escrow_pubkey = create_program_address(&[&["escrow"]], &escrow_program_id);

// construct a transfer message using that key
let message = Message::new(vec![
    token_instruction::transfer(&alice_pubkey, &escrow_pubkey, 1),
]);

// process the message which transfer one 1 token to the escrow
client.send_and_confirm_message(&[&alice_keypair], &message);
```

プログラムは同じ関数を使って同じアドレスを生成することができます。 以下の関数では、プログラムは、あたかもトランザクションに署名するための秘密鍵を持っているかのように、プログラムのアドレスから`token_instruction::transfer`を発行します。

```rust,ignore
fn transfer_one_token_from_escrow(
    program_id: &Pubkey,
    keyed_accounts: &[KeyedAccount]
) -> Result<()> {

    // User supplies the destination
    let alice_pubkey = keyed_accounts[1].unsigned_key();

    // Deterministically derive the escrow pubkey.
    let escrow_pubkey = create_program_address(&[&["escrow"]], program_id);

    // Create the transfer instruction
    let instruction = token_instruction::transfer(&escrow_pubkey, &alice_pubkey, 1);

    // The runtime deterministically derives the key from the currently
    // executing program ID and the supplied keywords.
    // If the derived address matches a key marked as signed in the instruction
    // then that key is accepted as signed.
    invoke_signed(&instruction,  &[&["escrow"]])?
}
```

### 特権が必要な説明

`create_program_address`で生成されたアドレスは、他の公開キーと見分けがつきません。 ランタイムがアドレスがプログラムのものであることを確認する唯一の方法は、プログラムがアドレスの生成に使用した種を提供することです。

ランタイムは内部で`create_program_address`を呼び出し、その結果を命令で与えられたアドレスと比較します。

## 例:

クロスプログラム呼び出しの使用方法の例については、 [ Rust](developing/deployed-programs/../../../deployed-programs/developing-rust.md#examples) と [C ](developing/deployed-programs/../../../deployed-programs/developing-c.md#examples)を使用した開発 を参照してください。
