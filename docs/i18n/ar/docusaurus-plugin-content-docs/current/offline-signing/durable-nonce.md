---
title: عدم وجود مُعاملات دائمة (Durable Transaction Nonces)
---

تُعد الأرقام الخاصة (nonces) المُستدامه للمُعاملة آلية للإلتفاف حول العمر القصير النموذجي لتجزئة الكتلة الحديثة للمُعاملة [`recent_blockhash`](developing/programming-model/transactions.md#recent-blockhash). يتم تنفيذها كبرنامج Solana، ويُمكن قراءة آلياته في المُقترح [proposal](../implemented-proposals/durable-tx-nonces.md).

## أمثلة الإستخدام (Usage Examples)

يُمكن العثور على تفاصيل الإستخدام الكاملة لأوامر الأرقام الخاصة (nounces) المُستدامه لـ CLI في المرجع [CLI reference](../cli/usage.md).

### سلطة الأرقام الخاصة المُستدامة (Nonce Authority)

يُمكن تعيين السلطة على حساب الأرقام الخاصة المُستدامة (nonce account) إختياريًا إلى حساب آخر. بذلك، ترث السلطة الجديدة السيطرة الكاملة على حساب الأرقام الخاصة المُستدامة (nonce account) من السلطة السابقة، بما في ذلك مُنشئ الحساب. تُتيح هذه الميزة إنشاء ترتيبات ملكية الحساب الأكثر تعقيدًا وعناوين الحسابات المُشتقة غير المُرتبطة بزوج المفاتيح (keypair). يتم إستخدام الوسيطة `--nonce-authority <AUTHORITY_KEYPAIR>` لتحديد هذا الحساب وتدعمها الأوامر التالية

- `create-nonce-account`
- `new-nonce`
- `withdraw-from-nonce-account`
- `authorize-nonce-account`

### إنشاء حساب الأرقام الخاصة المُستدامة (Nonce Account Creation)

تستخدم ميزة الرقم الخاص المُستدامه للمُعاملة (durable transaction nonce) حسابًا لتخزين القيمة nonce التالية. يجب أن تكون حسابات الأرقام الخاصة المُستدامة (nonce accounts) مُعفاة من الإيجار [rent-exempt](../implemented-proposals/rent.md#two-tiered-rent-regime)، لذلك يجب أن تحمل الحد الأدنى من الرصيد لتحقيق ذلك.

يتم إنشاء حساب أرقام خاصة مُستدامة (nonce account) عن طريق إنشاء زوج مفاتيح (keypair) جديد أولاً، ثم إنشاء الحساب على الشبكة

- الأمر البرمجي (Command)

```bash
solana-keygen new -o nonce-keypair.json
solana create-nonce-account nonce-keypair.json 1
```

- المُخرج (output)

```text
2SymGjGV4ksPdpbaqWFiDoBz8okvtiik4KE9cnMQgRHrRLySSdZ6jrEcpPifW4xUpp4z66XM9d9wM48sA7peG2XL
```

> للإحتفاظ بزوج المفاتيح (keypair) بلا إتصال بالأنترنات تمامًا، إستخدم تعليمات [instructions](wallet-guide/paper-wallet.md#seed-phrase-generation) إنشاء أزواج المفاتيح (keypair) في المحفظة الورقية [Paper Wallet](wallet-guide/paper-wallet.md) بدلاً من ذلك

> [وثائق الإستخدام الكاملة (Full usage documentation)](../cli/usage.md#solana-create-nonce-account)

### الإستعلام عن قيمة الرقم الخاص المُستدام المُخزن (Querying the Stored Nonce Value)

يتطلب إنشاء مُعاملة بالرقم الخاص المُستدام (durable nonce transaction) تمرير القيمة nonce المُخزنة كقيمة لتجزئة الكتلة `--blockhash` عند التوقيع والإرسال. أُحصل على قيمة الرقم الخاص المُستدام (nonce) المُخزن حاليًا مع

- الأمر البرمجي (Command)

```bash
solana nonce nonce-keypair.json
```

- المُخرج (output)

```text
8GRipryfxcsxN8mAGjy8zbFo9ezaUsh47TsPzmZbuytU
```

> [وثائق الإستخدام الكاملة (Full usage documentation)](../cli/usage.md#solana-get-nonce)

### الإستعلام عن قيمة الرقم الخاص المُستدام المُخزن (Querying the Stored Nonce Value)

بينما لا تكون هناك حاجة عادةً خارج مُعاملة أكثر فائدة، مكن تقديم قيمة الرقم الخاص المُستدام (nonce) المُخزن بواسطة

- الأمر البرمجي (Command)

```bash
solana new-nonce nonce-keypair.json
```

- المُخرج (output)

```text
44jYe1yPKrjuYDmoFTdgPjg8LFpYyh1PFKJqm5SC1PiSyAL8iw1bhadcAX1SL7KDmREEkmHpYvreKoNv6fZgfvUK
```

> [وثائق الإستخدام الكاملة (Full usage documentation)](../cli/usage.md#solana-new-nonce)

### حساب الرقم الخاص المُستدام (Display Nonce Account)

فحص حساب الرقم الخاص المُستدام (nonce account) بتنسيق أكثر مُلاءمة للإنسان بإستخدام

- الأمر البرمجي (Command)

```bash
solana nonce-account nonce-keypair.json
```

- المُخرج (output)

```text
balance: 0.5 SOL
minimum balance required: 0.00136416 SOL
nonce: DZar6t2EaCFQTbUP4DHKwZ1wT8gCPW2aRfkVWhydkBvS
```

> [وثائق الإستخدام الكاملة (Full usage documentation)](../cli/usage.md#solana-nonce-account)

### سحب الأموال من حساب الرقم الخاص المُستدام (Withdraw Funds from a Nonce Account)

سحب الأموال من حساب الرقم الخاص المُستدام (nonce account) بـ

- الأمر البرمجي (Command)

```bash
solana withdraw-from-nonce-account nonce-keypair.json ~/.config/solana/id.json 0.5
```

- المُخرج (output)

```text
3foNy1SBqwXSsfSfTdmYKDuhnVheRnKXpoPySiUDBVeDEs6iMVokgqm7AqfTjbk7QBE8mqomvMUMNQhtdMvFLide
```

> إغلاق حساب حساب الرقم الخاص المُستدام (nonce account) بسحب الرصيد الكامل

> [وثائق الإستخدام الكاملة (Full usage documentation)](../cli/usage.md#solana-withdraw-from-nonce-account)

### تعيين سلطة جديدة إلى حساب الرقم الخاص المُستدام (Assign a New Authority to a Nonce Account)

إعادة تعيين سلطة حساب الرقم الخاص المُستدام (nonce account) بعد الإنشاء بـ

- الأمر البرمجي (Command)

```bash
solana authorize-nonce-account nonce-keypair.json nonce-authority.json
```

- المُخرج (output)

```text
3F9cg4zN9wHxLGx4c3cUKmqpej4oa67QbALmChsJbfxTgTffRiL3iUehVhR9wQmWgPua66jPuAYeL1K2pYYjbNoT
```

> [وثائق الإستخدام الكاملة (Full usage documentation)](../cli/usage.md#solana-authorize-nonce-account)

## أوامر أخرى داعمة للأرقام الخاصة المُستدامة (Other Commands Supporting Durable Nonces)

للإستفادة من الأرقام الخاصة المُستدامة (nonces) مع أوامر CLI الفرعية الأخرى، يجب دعم وسيطين.

- يُحدد `--nonce` الحساب الذي يُخزن قيمة الرقم الخاص المُستدام (nonce)
- `--nonce-authority` ، يُحدد [nonce authority](#nonce-authority) إختياري

تلقت الأوامر الفرعية التالية هذا العلاج حتى الآن

- [`pay`](../cli/usage.md#solana-pay)
- [`delegate-stake`](../cli/usage.md#solana-delegate-stake)
- [`deactivate-stake`](../cli/usage.md#solana-deactivate-stake)

### مثال دفع بإستخدام الرقم الخاص المُستدام (Example Pay Using Durable Nonce)

نوضح هنا كيف أن أليس تدفع عدد 1 SOL لـ Bob بإستخدام رقم خاص مُستدام (nonce). الإجراء هو نفسه لجميع الأوامر الفرعية التي تدعم الأرقام الخاصة المُستدامة (nonces)

#### - إنشاء حسابات (Create accounts)

نحتاج أولاً إلى بعض الحسابات الخاصة بـ Alice والرقم الخاص المُستدام (nonce) لـ Alice و Bob

```bash
$ solana-keygen new -o alice.json
$ solana-keygen new -o nonce.json
$ solana-keygen new -o bob.json
```

#### - تمويل حساب أليس (Fund Alice's account)

ستحتاج Alice إلى بعض الأموال لإنشاء حساب الرقم الخاص المُستدام (nonce account) وإرساله إلى Bob. قُم بإرسال بعض عملات SOL بالتوزيع الحر (AiDrop)

```bash
$ solana airdrop -k alice.json 1
1 SOL
```

#### - إنشاء حساب الرقم الخاص المُستدام الخاص بـأليس (Create Alice's nonce account)

تحتاج Alice الآن إلى حساب الرقم الخاص المُستدام (nonce account). إنشاء واحد

> لا يتم هنا إستخدام [nonce authority](#nonce-authority) مُنفصل، لذلك يتمتع `alice.json` بالسلطة الكاملة على حساب الرقم الخاص المُستدام (nonce account)

```bash
$ solana create-nonce-account -k alice.json nonce.json 0.1
3KPZr96BTsL3hqera9up82KAU462Gz31xjqJ6eHUAjF935Yf8i1kmfEbo6SVbNaACKE5z6gySrNjVRvmS8DcPuwV
```

#### - فشل أول مُحاولة للدفع لـ "بوب" (A failed first attempt to pay Bob)

تحاول Alice الدفع لـ Bob، ولكنها تستغرق وقتاً طويلاً جداً للتوقيع. تنتهي صلاحية تجزئة الكتلة (Blockhash) المُحددة وتفشل الةعاملة

```bash
$ solana pay -k alice.json --blockhash expiredDTaxfagttWjQweib42b6ZHADSx94Tw8gHx3W7 bob.json 0.01
[2020-01-02T18:48:28.462911000Z ERROR solana_cli::cli] Io(Custom { kind: Other, error: "Transaction \"33gQQaoPc9jWePMvDAeyJpcnSPiGUAdtVg8zREWv4GiKjkcGNufgpcbFyRKRrA25NkgjZySEeKue5rawyeH5TzsV\" failed: None" })
Error: Io(Custom { kind: Other, error: "Transaction \"33gQQaoPc9jWePMvDAeyJpcnSPiGUAdtVg8zREWv4GiKjkcGNufgpcbFyRKRrA25NkgjZySEeKue5rawyeH5TzsV\" failed: None" })
```

#### - الرقم الخاص المُستدام اللإنقاذ! (Nonce to the rescue)!

تُعيد Alice مُحاولة إرسال المُعاملة، ولكن هذه المرة تُحدد حسابها حساب الرقم الخاص المُستدام (nonce account) وتجزئة الكتلة (Blockhash) المُخزنة هناك

> تذكر، `alice.json` هو [nonce authority](#nonce-authority) في هذا المثال

```bash
$ solana nonce-account nonce.json
balance: 0.1 SOL
minimum balance required: 0.00136416 SOL
nonce: F7vmkY3DTaxfagttWjQweib42b6ZHADSx94Tw8gHx3W7
```

```bash
$ solana pay -k alice.json --blockhash F7vmkY3DTaxfagttWjQweib42b6ZHADSx94Tw8gHx3W7 --nonce nonce.json bob.json 0.01
HR1368UKHVZyenmH7yVz5sBAijV6XAPeWbEiXEGVYQorRMcoijeNAbzZqEZiH8cDB8tk65ckqeegFjK8dHwNFgQ
```

#### - نجاح (Success)!

نجاح المُعاملة! يتلقى Bob عدد 0.01 SOL من Alice والرقم الخاص المُستدام (nonce) المُخزن الخاص بـAlice يتقدم إلى قيمة جديدة

```bash
$ solana balance -k bob.json
0.01 SOL
```

```bash
$ solana nonce-account nonce.json
balance: 0.1 SOL
minimum balance required: 0.00136416 SOL
nonce: 6bjroqDcZgTv6Vavhqf81oBHTv3aMnX19UTB51YhAZnN
```
