---
title: توقيع المُعاملات دون راجعتصال
---

تتطلب بعض نماذج الأمان الإحتفاظ بمفاتيح التوقيع، وبالتالي عملية التوقيع، مُنفصلة عن إنشاء المُعاملات والبث الشبكي. تشمل الأمثلة ما يلي:

- جمع التوقيعات من الموقعين المتباينين جغرافيا في مخطط متعدد التوقيعات [multi-signature scheme](cli/usage.md#multiple-witnesses)
- عمليات التوقيع بإستخدام فجوة الهواء [airgapped](https://en.wikipedia.org/wiki/Air_gap_(networking)) جهاز تسجيل غير مُتصل بالأنترنات

يصف هذا المُستند كيفية إستخدام Solana's CLI لتوقيع وتسليم العملية بشكل مُنفصل.

## الأوامر التي تدعم التوقيع بدون إتصال

الأوامر التالي تدعم التوقيع دون اتصال في الوقت الحالي:

- [`إنشاء حساب إثبات الحصة`](cli/usage.md#solana-create-stake-account)
- [`تعطيل الحصة`](cli/usage.md#solana-deactivate-stake)
- [`تفويض الحصة`](cli/usage.md#solana-delegate-stake)
- [`تقسيم الحصة`](cli/usage.md#solana-split-stake)
- [`تفويض الحصة`](cli/usage.md#solana-stake-authorize)
- [`قفل مجموعة الحصة`](cli/usage.md#solana-stake-set-lockup)
- [`تحويل`](cli/usage.md#solana-transfer)
- [`سحب حصة`](cli/usage.md#solana-withdraw-stake)

## توقيع المُعاملات دون إتصال

لتوقيع مُعاملة بدون إتصال، قم بتمرير الأسباب التالية إلى سطر الأوامر

1. `--sign-only` يمنع العميل من تسليم عملية مسجلة إلى الشبكة. ويتم عرض زوجي المفتاح العام والتوقيع بدلًا من ذلك في المخرج القياسي.
2. `--blockhash BASE58_HASH`, يسمح للمتصل بتحديد القيمة المستخدمة لتعبئة حقل العملية `recent_blockhash`. ويخدم هذا الأمر عددًا من الأغراض، وتحديدًا: _ يلغي الحاجة إلى الاتصال بالشبكة ويستعلم عن blockhash حديث من خلال RPC _ يمكّن الموقعين من تنسيق blockhash بمخطط متعدد التوقيعات

### مثال: توقيع دفعة دون إتصال

أمر

```bash
solana@offline$ solana pay --sign-only --blockhash 5Tx8F3jgSHx21CbtjwmdaKPLM5tWmreWAnPrbqHomSJF \
    recipient-keypair.json 1
```

الناتج

```text

Blockhash: 5Tx8F3jgSHx21CbtjwmdaKPLM5tWmreWAnPrbqHomSJF
Signers (Pubkey=Signature):
  FhtzLVsmcV7S5XqGD79ErgoseCLhZYmEZnz9kQg1Rp7j=4vC38p4bz7XyiXrk6HtaooUqwxTWKocf45cstASGtmrD398biNJnmTcUCVEojE7wVQvgdYbjHJqRFZPpzfCQpmUN

{"blockhash":"5Tx8F3jgSHx21CbtjwmdaKPLM5tWmreWAnPrbqHomSJF","signers":["FhtzLVsmcV7S5XqGD79ErgoseCLhZYmEZnz9kQg1Rp7j=4vC38p4bz7XyiXrk6HtaooUqwxTWKocf45cstASGtmrD398biNJnmTcUCVEojE7wVQvgdYbjHJqRFZPpzfCQpmUN"]}'
```

## تقديم معاملات مُوقّعة دون إتصال إلى الشبكة

لتقديم معاملة تم توقيعها دون اتصال إلى الشبكة، عليك تمرير الحجج التالية إلى سطر الأوامر

1. `--blockhash BASE58_HASH`, يجب أن يكون نفس تجزئة الكُتلة (Blockhash) كما تم استخدامه في التوقيع
2. `--signer BASE58_PUBKEY=BASE58_SIGNATURE`, واحد لكل موقع دون إتصال. هذا يشمل زوجي المفتاح العام والتوقيع في العملية بدلًا من توقيعها مع أي زوج مفاتيح (أو أزواج مفاتيح) محلية

### مثال: تقديم دفعة مُوقّعة دون إتصال

أمر

```bash
solana@online$ solana pay --blockhash 5Tx8F3jgSHx21CbtjwmdaKPLM5tWmreWAnPrbqHomSJF \
    --signer FhtzLVsmcV7S5XqGD79ErgoseCLhZYmEZnz9kQg1Rp7j=4vC38p4bz7XyiXrk6HtaooUqwxTWKocf45cstASGtmrD398biNJnmTcUCVEojE7wVQvgdYbjHJqRFZPpzfCQpmUN
    recipient-keypair.json 1
```

الناتج

```text
4vC38p4bz7XyiXrk6HtaooUqwxTWKocf45cstASGtmrD398biNJnmTcUCVEojE7wVQvgdYbjHJqRFZPpzfCQpmUN
```

## توقيع بدون إتصال عبر جلسات مُتعددة

يمكن أن يحدث التوقيع دون إتصال عبر جلسات مُتعددة. في هذا السيناريو، قم بتمرير المفتاح العام المفقود للموقع لكل دور. جميع المفاتيح العامة التي تم تحديدها، ولكن لم يتم إنشاء توقيع لها سيتم إضافتها كمفقودة في نتائج التوقيع دون اتصال

### مثال: التحويل خلال جلستي توقيع دون إإتصال

أمر (جلسة رقم 1 دون إتصال)

```text
solana@offline1$ solana transfer Fdri24WUGtrCXZ55nXiewAj6RM18hRHPGAjZk3o6vBut 10 \
    --blockhash 7ALDjLv56a8f6sH6upAZALQKkXyjAwwENH9GomyM8Dbc \
    --sign-only \
    --keypair fee_payer.json \
    --from 674RgFMgdqdRoVtMqSBg7mHFbrrNm1h1r721H1ZMquHL
```

الناتج (جلسة رقم 1 دون اتصال)

```text
Blockhash: 7ALDjLv56a8f6sH6upAZALQKkXyjAwwENH9GomyM8Dbc
Signers (Pubkey=Signature):
  3bo5YiRagwmRikuH6H1d2gkKef5nFZXE3gJeoHxJbPjy=ohGKvpRC46jAduwU9NW8tP91JkCT5r8Mo67Ysnid4zc76tiiV1Ho6jv3BKFSbBcr2NcPPCarmfTLSkTHsJCtdYi
Absent Signers (Pubkey):
  674RgFMgdqdRoVtMqSBg7mHFbrrNm1h1r721H1ZMquHL
```

أمر (جلسة رقم 2 دون اتصال)

```text
solana@offline2$ solana transfer Fdri24WUGtrCXZ55nXiewAj6RM18hRHPGAjZk3o6vBut 10 \
    --blockhash 7ALDjLv56a8f6sH6upAZALQKkXyjAwwENH9GomyM8Dbc \
    --sign-only \
    --keypair from.json \
    --fee-payer 3bo5YiRagwmRikuH6H1d2gkKef5nFZXE3gJeoHxJbPjy
```

ناتج (جلسة رقم 2 دون اتصال)

```text
Blockhash: 7ALDjLv56a8f6sH6upAZALQKkXyjAwwENH9GomyM8Dbc
Signers (Pubkey=Signature):
  674RgFMgdqdRoVtMqSBg7mHFbrrNm1h1r721H1ZMquHL=3vJtnba4dKQmEAieAekC1rJnPUndBcpvqRPRMoPWqhLEMCty2SdUxt2yvC1wQW6wVUa5putZMt6kdwCaTv8gk7sQ
Absent Signers (Pubkey):
  3bo5YiRagwmRikuH6H1d2gkKef5nFZXE3gJeoHxJbPjy
```

أمر (تقديم عبر الإنترنت)

```text
solana@online$ solana transfer Fdri24WUGtrCXZ55nXiewAj6RM18hRHPGAjZk3o6vBut 10 \
    --blockhash 7ALDjLv56a8f6sH6upAZALQKkXyjAwwENH9GomyM8Dbc \
    --from 674RgFMgdqdRoVtMqSBg7mHFbrrNm1h1r721H1ZMquHL \
    --signer 674RgFMgdqdRoVtMqSBg7mHFbrrNm1h1r721H1ZMquHL=3vJtnba4dKQmEAieAekC1rJnPUndBcpvqRPRMoPWqhLEMCty2SdUxt2yvC1wQW6wVUa5putZMt6kdwCaTv8gk7sQ \
    --fee-payer 3bo5YiRagwmRikuH6H1d2gkKef5nFZXE3gJeoHxJbPjy \
    --signer 3bo5YiRagwmRikuH6H1d2gkKef5nFZXE3gJeoHxJbPjy=ohGKvpRC46jAduwU9NW8tP91JkCT5r8Mo67Ysnid4zc76tiiV1Ho6jv3BKFSbBcr2NcPPCarmfTLSkTHsJCtdYi
```

ناتج (تقديم الإنترنت)

```text
ohGKvpRC46jAduwU9NW8tP91JkCT5r8Mo67Ysnid4zc76tiiV1Ho6jv3BKFSbBcr2NcPPCarmfTLSkTHsJCtdYi
```

## كسب المزيد من الوقت للتوقيع

ويجب في العادة أن يتم توقيع وقبول معاملة Solana من قبل الشبكة خلال عدد من الفتحات من blockhash في `recent_blockhash` حقل (دقيقتان تقريبًا من وقت هذه الكتابة). إذا استغرقت عملية توقيعك مدة أطول من ذلك، فإن [Durable Transaction Nonce](offline-signing/durable-nonce.md) بإمكانه إعطاءك الوقت الإضافية اللازم.
