---
title: "概要"
---

開発者は、自分のプログラムを書いて Solana ブロックチェーンにデプロイすることができます。

[Helloworld の例](examples.md#helloworld) は、 プログラムがどのように書き込まれ、構築され、デプロイされ、そしてチェーン上でどのように動作するかを見るための良い出発点です。

## Berkley Packet Filter (BPF)

Solana のオンチェーンプログラムは、[LLVM compiler infrastructure](https://llvm.org/)を経由して、 [Berkley Packet Filter (BPF)](https://en.wikipedia.org/wiki/Berkeley_Packet_Filter) のバイトコードのバリエーションを含む[Executable and Linkable Format (ELF)](https://en.wikipedia.org/wiki/Executable_and_Linkable_Format) にコンパイルされます。

Solana は LLVM コンパイラインフラストラクチャを使用するため、プログラムは、LLVM の BPF バックエンドをターゲットとするあらゆるプログラミング言語で 記述することができます。 Solana は現在、Rust と C/C++ でプログラムを書くことをサポートしています。

BPF は効率的な[命令セット](https://github.com/iovisor/bpf-docs/blob/master/eBPF.md)を提供しており、インタプリタ型仮想マシンで実行したり、効率的なジャストインタイムコンパイルのネイティブ命令として実行することができます。

## メモリマップ

Solana BPF プログラムで使用されている仮想アドレスメモリマップは、以下のように修正されます。

- プログラムコードは 0x100000000 から始まります
- スタックデータは 0x200000000 から始まります
- ヒープデータは 0x30000000 から始まります
- プログラムの入力パラメータは 0x4000000 から始まります

上記の仮想アドレスはスタートアドレスですが、プログラムにはメモリマップのサブセットへのアクセスが与えられています アクセスが許可されていない仮想アドレスに読み書きしようとすると、プログラムはパニックに陥り、違反しようとしたアドレスとサイズを含む`AccessViolation`エラーが返されます。

## スタック

BPF では、可変スタックポインタの代わりにスタックフレームを使用しています。 各スタックフレームのサイズは 4KB です。

プログラムがこのスタックフレームのサイズに違反した場合、コンパイラはオーバーランを警告として報告します。

For example: `Error: Function _ZN16curve25519_dalek7edwards21EdwardsBasepointTable6create17h178b3d2411f7f082E Stack offset of -30728 exceeded max offset of -4096 by 26632 bytes, please minimize large stack variables`

このメッセージは、どのシンボルがスタックフレームを超えているかを示していますが、Rust や C++のシンボルの場合、名前が混乱している可能性があります。 Rust シンボルをデアングルするには、 [rustfilt](https://github.com/luser/rustfilt) を使用します。 上記の警告は Rust プログラムから出てきたので、解体されたシンボル名は次のとおりです。

```bash
$ rustfilt _ZN16curve25519_dalek7edwards21EdwardsBasepointTable6create17h178b3d2411f7f082E
curve25519_dalek::edwards::EdwardsBasepointTable::create
```

C++シンボルを解除するには、binutils の`c++filt`を使用します。

エラーではなく警告が報告されるのは、プログラムがその機能を使用していなくても、依存しているクレートにスタックフレームの制限に違反する機能が含まれている場合があるからです。 プログラムが実行時にスタックサイズに違反した場合、`AccessViolation`エラーが報告されます。

BPF のスタックフレームは、0x200000000 から始まる仮想アドレス範囲を占めます。

## コールの深さ

プログラムは迅速に実行するように制限されており、これを容易にするために、プログラムの コールスタックは最大 64 フレームに制限されています。

## ヒープ

プログラムは、Rust `alloc` API を介して直接実行時ヒープにアクセスできます。 高速割り当てを容易にするために、シンプルな 32KB のバンプヒープが 利用されています。 ヒープは `free` または `realloc` をサポートしていないため、賢明に使用してください。

内部的に プログラムは、仮想 アドレス 0x30000000 から始まる 32KB のメモリ領域にアクセスでき、プログラムの 固有のニーズに基づいてカスタムヒープを実装することができます。

- [Rust program heap usage](developing-rust.md#heap)
- [C program heap usage](developing-c.md#heap)

## フロートサポート

Programs support a limited subset of Rust's float operations, if a program attempts to use a float operation that is not supported, the runtime will report an unresolved symbol error.

Float operations are performed via software libraries, specifically LLVM's float builtins. Due to be software emulated they consume more compute units than integer operations. In general, fixed point operations are recommended where possible.

The Solana Program Library math tests will report the performance of some math operations: https://github.com/solana-labs/solana-program-library/tree/master/libraries/math

To run the test, sync the repo, and run:

`$ cargo test-bpf -- --nocapture --test-threads=1`

Recent results show the float operations take more instructions compared to integers equivalents. Fixed point implementations may vary but will also be less then the float equivalents:

```
         u64   f32
Multipy    8   176
Divide     9   219
```

## 静的書き込み可能なデータ

プログラムの共有オブジェクトは、書き込み可能な共有データをサポートしていません。 プログラムは、複数の並列実行の間で、同じ読み取り専用の共有コードとデータを使って共有されます。 つまり、開発者はプログラムに静的な書き込み可能な変数やグローバル変数を含めるべきではありません。 将来的には、書き込み可能なデータをサポートするために、Copy-on-Write メカニズムが追加されるかもしれません。

## 署名された除算

BPF 命令セットは、 [signed Division](https://www.kernel.org/doc/html/latest/bpf/bpf_design_QA.html#q-why-there-is-no-bpf-sdiv-for-signed-divide-operation) をサポートしていません。 符号付き除算命令の追加は検討課題です。

## ローダ

プログラムはランタイムローダーによってデプロイされ、実行されます。 現在、 サポートされている 2 つのローダー [BPF ローダー](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/bpf_loader.rs#L17)と [BPF ローダー 非推奨](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/bpf_loader_deprecated.rs#L14)

ローダーは異なるアプリケーション・バイナリ・インターフェースをサポートしている場合があるため、開発者は同じローダー用にプログラムを書き、同じローダーにデプロイする必要があります。 あるローダー用に書かれたプログラムを別のローダーにデプロイした場合、プログラムの入力パラメータのデシリアライズが一致しないため、通常は`AccessViolation`エラーが発生します。

実用的な目的のために、プログラムは常に最新の BPF ローダーをターゲットにして書かれるべきであり、"コマンドラインインターフェース"と"javascript API"のデフォルトは最新のローダーです。

特定のローダー用のプログラムの実装に関する言語固有の情報については、以下を参照してください。

- [Rust program entrypoints](developing-rust.md#program-entrypoint)
- [C program entrypoints](developing-c.md#program-entrypoint)

### デプロイ

BPF プログラムの展開は、BPF 共有オブジェクトをプログラムアカウントのデータにアップロードし、アカウントを実行可能にするプロセスです。 クライアントは、BPF 共有オブジェクトを小さいピースに分割し、ローダーがそのデータをプログラムのアカウントデータに書き込む[``](https://github.com/solana-labs/solana/blob/bc7133d7526a041d1aaee807b80922baa89b6f90/sdk/program/src/loader_instruction.rs#L13)命令をローダーに送信します。そこで、ローダーはそのデータをプログラムのアカウントデータに書き込みます。 すべてのピースを受信したら、クライアントは [`Finalize`](https://github.com/solana-labs/solana/blob/bc7133d7526a041d1aaee807b80922baa89b6f90/sdk/program/src/loader_instruction.rs#L30)命令をローダーに送ると、ローダーは BPF データが有効であることを検証し、プログラムアカウントを*実行可能*とマークします。 すべてのピースを受け取ったら、クライアントはローダーに Finalize 命令を送り、ローダーは BPF データが有効であることを検証し、プログラムアカウントを実行可能とマークします。ローダーは BPF データの有効性を確認し、プログラムアカウントを実行可能とマークします。

ローダーは、実行可能な BPF プログラムに命令が送られると、プログラムの実行環境を設定し、プログラムの入力パラメーターをシリアル化し、プログラムのエントリーポイントを呼び出し、エラーが発生した場合は報告します。

詳細については、 [デプロイ](deploying.md) を参照してください。

### Input Parameter Serialization

BPF ローダーは、プログラムの入力パラメータをバイト配列にシリアライズし、それをプログラムのエントリーポイントに渡し、プログラムはそれをオンチェーンでデシリアライズする責任を負います。 非推奨のローダーと現在のローダーとの間の変更点の 1 つは、入力パラメータが直列化され、様々なパラメータが整列化されたバイト配列内の整列されたオフセット上に配置されることです。 これにより、デシリアライゼーションの実装では、バイト配列を直接参照し、プログラムにアラインドされたポインタを提供することができます。

シリアル化に関する言語固有の情報については以下を参照してください:

- [Rust program parameter deserialization](developing-rust.md#parameter-deserialization)
- [C program parameter deserialization](developing-c.md#parameter-deserialization)

最新のローダーはプログラム入力パラメータを次のようにシリアル化します(すべての エンコードは少しエンディアンです):

- 8 バイトの符号なしアカウント
- 各アカウントに対して
  - 重複していない場合は 0xff、重複している場合は重複しているアカウントのインデックスを示す 1 バイト。
  - 7 バイトのパディング
    - 重複しない場合
      - 1 バイトのパディング
      - 1 バイトの真偽値、アカウントが署名者の場合は true
      - 1 バイトの真偽値、アカウントが書き込み可能であれば true。
      - 1 バイトの真偽値、アカウントが署名者の場合は true
      - 4 バイトのパディング
      - アカウントの公開キーの 32 バイト
      - アカウントの公開キーの 32 バイト
      - アカウントが所有する 8 バイトの未署名のラムポート数
      - 8 バイトのアカウントデータの符号なしバイト数
      - x バイトのアカウントデータ
      - 10k バイトのパディング、realloc に使用されます
      - オフセットを 8 バイトに整列させるのに十分なパディング。
      - 8 bytes レンタルエポック
- 8 バイトの符号なし命令データの数
- x バイトの命令データ
- 32 バイトのプログラム ID
