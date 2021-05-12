---
title: "أمثلة"
---

## مرحبا أيها العالم (Helloworld)

مرحبا أيها العالم أو Hello World هو مشروع يُوضح كيفية إستخدام جافاسكريبت (Javascript) واجهة برمجة تطبيقات (API) الخاصة بSolana وكل من اللغة البرمجية Rust و C لبناء البرامج ونشرها والتفاعل معها على بلوكشاين Solana.

يشمل المشروع ما يلي:

- برنامج مرحبا أيها العالم أو Hello world على الشبكة (on-chain)
- عميل (client) يمكنه إرسال "مرحبًا" إلى حساب ما وإستعادة عدد مرات إرسال "مرحبًا"

### البناء والتشغيل

قم أولاً بإحضار أحدث إصدار من رمز الكود (code):

```bash
$ git clone https://github.com/solana-labs/example-helloworld.git
$ cd example-helloworld
```

بعد ذلك، إتبع الخطوات في مستودع git [README](https://github.com/solana-labs/example-helloworld/blob/master/README.md).

## إستراحة

تطبيق [Break](https://break.solana.com/) هو تطبيق تفاعلي (React app) يمنح المستخدمين شعورًا عميقًا بمدى سرعة شبكة Solana وأدائها العالي. هل يمكنك كسر _break_ شبكة بلوكشاين؟ أثناء اللعب لمدة 15 ثانية، تُرسل كل نقرة على زر أو ضغطة مفتاح معاملة جديدة إلى المجموعة (cluster). قم بتحطيم لوحة المفاتيح بأسرع ما يمكنك وشاهد معاملاتك يتم الإنتهاء منها في الوقت الفعلي بينما تأخذ الشبكة كل شيء على قدم وساق!

يمكن تشغيل Break على شبكات Devnet و Testnet و Mainnet Beta الخاصة بنا. عدد مرات اللعب مجانية على Devnet و Testnet، حيث يتم تمويل الجلسة من خلال صنبور الشبكة. على Mainnet Beta، يدفع المُستخدمون للعب 0.08 SOL لكل لعبة. يُمكن تمويل حساب الجلسة من خلال محفظة تخزين مفاتيح محلية أو عن طريق مسح رمز QR من Trust Wallet لنقل الرموز.

[أنقر هنا للعب Break](https://break.solana.com/)

### البناء والتشغيل

قم أولاً بإحضار أحدث إصدار من رمز الكود (code):

```bash
$ git clone https://github.com/solana-labs/break.git
$ cd break
```

بعد ذلك، إتبع الخطوات في مستودع git من هنا [README](https://github.com/solana-labs/break/blob/master/README.md).

## اللغة المُحددة

- [Rust](developing-rust.md#examples)
- [C](developing-c.md#examples)
