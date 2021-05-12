---
title: الرقم الخاص المُستدام للمُعاملات (Durable Transaction Nonces)
---

## المُشكل (Problem)

لمنع إعادة التشغيل، تحتوي مُعاملات Solana على حقل الرقم الخاص المُستدام (nonce) يتم ملؤه بقيمة تجزئة الكتلة (Blockhash) "الأخيرة". تم رفض مُعاملة تحتوي على تجزئة كتلة (blockhash) قديم جدًا (حوالي دقيقتان حتى كتابة هذه السطور) من قبل الشبكة بإعتبارها غير صالحة. لسوء الحظ، تتطلب بعض حالات الإستخدام، مثل خدمات الحراسة، مزيدًا من الوقت لإنتاج توقيع للمُعاملة. هناك حاجة إلى آلية لتمكين هؤلاء المشاركين غير المتصلين بالشبكة.

## المُتطلبات (Requirements)

1. يجب أن يغطي توقيع المُعاملة قيمة الرقم الخاص المُستدام (nonce)
2. يجب ألا يكون الرقم الخاص المُستدام (nonce) قابلاً لإعادة الإستخدام، حتى في حالة توقيع الكشف عن مفتاح

## حل قائم على العقد (A Contract-based Solution)

هنا نصف حلاً قائمًا على العقد للمُشكلة، حيث يمكن للعميل "إخفاء" قيمة الرقم الخاص المُستدام (nonce) للإستخدام المُستقبلي في حقل تجزئة الكتلة الحديثة `recent_blockhash` للمُعاملة. يشبه هذا النهج تعليمة المُقارنة والمُبادلة الذرية (Swap atomic)، التي تنفذها بعض ISAs لوحدة المُعالجة المركزية.

عند إستخدام الرقم الخاص المُستدام (nonce) دائم، يجب على العميل أولاً الإستعلام عن قيمته من بيانات الحساب. يتم الآن إنشاء المُعاملة بالطريقة العادية، ولكن مع المُتطلبات الإضافية التالية:

1. يتم إستخدام قيمة الـ nonce الدائمة في الحقل تجزئة الكتلة الحديثة `recent_blockhash`
2. يُعد أمر `AdvanceNonceAccount` هو أول أمر يتم إصداره في المُعاملة

### ميكانيكا العقود (Contract Mechanics)

للقيام بذلك: svgbob هذا في مُخطط إنسيابي

```text
Start
Create Account
  state = Uninitialized
NonceInstruction
  if state == Uninitialized
    if account.balance < rent_exempt
      error InsufficientFunds
    state = Initialized
  elif state != Initialized
    error BadState
  if sysvar.recent_blockhashes.is_empty()
    error EmptyRecentBlockhashes
  if !sysvar.recent_blockhashes.contains(stored_nonce)
    error NotReady
  stored_hash = sysvar.recent_blockhashes[0]
  success
WithdrawInstruction(to, lamports)
  if state == Uninitialized
    if !signers.contains(owner)
      error MissingRequiredSignatures
  elif state == Initialized
    if !sysvar.recent_blockhashes.contains(stored_nonce)
      error NotReady
    if lamports != account.balance && lamports + rent_exempt > account.balance
      error InsufficientFunds
  account.balance -= lamports
  to.balance += lamports
  success
```

يبدأ العميل الذي يرغب في إستخدام هذه الميزة بإنشاء حساب الرقم الخاص المُستدام (nonce) ضمن برنامج النظام. سيكون هذا الحساب في حالة غير مُهيئة `Uninitialized` بدون تجزئة (hash) مخزنة، وبالتالي غير قابل للإستخدام.

لتهيئة حساب تم إنشاؤه حديثًا، يجب إصدار أمر `InitializeNonceAccount`. تأخذ هذه التعليمات مُعلمة (parameter) واحدة، المفتاح العمومي `Pubkey` لحساب [authority](../offline-signing/durable-nonce.md#nonce-authority). يجب أن تكون حسابات الرقم الخاص المُستدام (nonce) مُعفاة من الإيجار [rent-exempt](rent.md#two-tiered-rent-regime) لتوفي بمتطلبات إستمرارية البيانات للميزة، وعلى هذا النحو، تتطلب إيداع lamports كافية قبل أن يتم تهيئتها. عند التهيئة الناجحة، يتم تخزين أحدث تجزئة كتلة (Blockhash) جنبًا إلى جنب مع سلطة الـ nonce المفتاح العمومي `Pubkey` المُحددة.

تُستخدم تعليمات `AdvanceNonceAccount` لإدارة قيمة الرقم الخاص المُستدام (nonce) المُخزنة في الحساب. تُخزن أحدث تجزئة للكتلة (Blockhash) للكتلة في بيانات حالة الحساب، ويفشل إذا كان ذلك يتطابق مع القيمة المُخزنة بالفعل هناك. يمنع هذا الإختيار إعادة تشغيل المُعاملات داخل نفس الكتلة (block).

نظرًا لمُتطلبات nonce حسابات المُعفاة من الإيجار [rent-exempt](rent.md#two-tiered-rent-regime)، يتم إستخدام تعليمات سحب مُخصصة لنقل الأموال خارج الحساب. تأخذ تعليمات `WithdrawNonceAccount` وسيطة واحدة وسحب الـ lamports وتفرض الإعفاء من الإيجار (rent-exemption) عن طريق منع رصيد الحساب من الإنخفاض إلى أقل من الحد الأدنى للإعفاء من الإيجار. الإستثناء من هذا الفحص هو ما إذا كان الرصيد النهائي سيكون صفراً من الـ lamports، مما يجعل الحساب مُؤهلاً للحذف. تحتوي تفاصيل إغلاق الحساب هذه على مطلب إضافي مفاده أن قيمة الـ nonce المُخزنة يجب ألا تتطابق مع تجزئة الكتلة (Blockhash) الأحدث للكتلة، وفقًا لـ `AdvanceNonceAccount`.

يمكن تغيير الحساب [nonce authority](../offline-signing/durable-nonce.md#nonce-authority) بإستخدام التعليمات `AuthorizeNonceAccount`. يتطلب مُعلِّمة (parameter) واحدة، المفتاح العمومي `Pubkey` للسلطة الجديدة. يمنح تنفيذ هذه التعليمات سيطرة كاملة على الحساب ورصيده للسلطة الجديدة.

> تتطلب كل من `AdvanceNonceAccount` ، `WithdrawNonceAccount` و `AuthorizeNonceAccount` الرقم الحالي [nonce authority](../offline-signing/durable-nonce.md#nonce-authority) للحساب لتوقيع المُعاملة.

### دعم وقت التشغيل (Runtime Support)

العقد (contract) وحده لا يكفي لتنفيذ هذه الميزة. لفرض وجود `recent_blockhash` على المُعاملة ومنع سرقة الرسوم عبر إعادة تشغيل المُعاملة الفاشلة، تُعد تعديلات وقت التشغيل ضرورية.

سيتم إختبار أي مُعاملة تفشل في التحقق المُعتاد من عمر التجزئة `check_hash_age` للحصول على Nonce مُعاملة متينة. تتم الإشارة إلى ذلك من خلال تضمين تعليمات `AdvanceNonceAccount` كأول تعليمات في المُعاملة.

إذا حدد وقت التشغيل أن المُعاملات المتينة قيد الإستخدام، فسيتخذ الإجراءات الإضافية التالية للتحقق من صحة المُعاملة:

1. تم تحميل `NonceAccount` المُحدد في تعليمات `Nonce`.
2. تم إلغاء تسلسل `NonceState` من حقل البيانات `NonceAccount` والتأكيد على أنها في حالة `Initialized`.
3. يتم إختبار القيمة nonce المُخزنة في `NonceAccount` لمُطابقة القيمة المُحددة في الحقل `recent_blockhash` للمُعاملة.

في حالة نجاح عمليات التحقق الثلاثة المذكورة أعلاه ، يُسمح للمُعاملة بمُواصلة التحقق من المُصادقة (validation).

نظرًا لأن المُعاملات التي تفشل بإستخدام `InstructionError` يتم تحصيل رسوم منها والتراجع عن التغييرات التي تطرأ على حالتها، فهناك فرصة لسرقة الرسوم إذا تم إرجاع تعليمات `AdvanceNonceAccount`. يمكن أن يعيد المُدقّق (validator) الضار تشغيل المُعاملة الفاشلة حتى يتم التقدم بنجاح في الـ nonce المُخزنة. تمنع تغييرات وقت التشغيل هذا السلوك. عندما تفشل مُعاملة غير مُتوقعة دائمة (durable nonce transaction) مع `InstructionError` بصرف النظر عن التعليمات `AdvanceNonceAccount`، يتم إرجاع الحساب nonce إلى حالة ما قبل التنفيذ كالمُعتاد. ثم يقوم وقت التشغيل بتقديم قيمة الـ nonce الخاصة به ويتم تخزين حساب الـ nonce المُتقدم كما لو كان قد نجح.
