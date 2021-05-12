---
title: إنشاء المفتاح العمومي للمُدقّق (Create a validator public key)
---

من أجل المُشاركة، عليك التسجيل أولاً. راجع معلومات التسجيل [Registration info](../registration/how-to-register.md).

من أجل الحصول على تخصيص عملة SOL الخاصة بك، تحتاج إلى نشر مفتاح الهوية العمومي للمُدقّق (validator's identity public key) الخاص بك تحت حساب keybase.io الخاص بك.

## **توليد زوج المفاتيح (Generate Keypair)**

1. إذا لم تكن قد قُمت بذلك بالفعل، فقُم بإنشاء زوج مفاتيح هوية المُدقّق (validator identity keypair) عن طريق تشغيل:

   ```bash
     solana-keygen new -o ~/validator-keypair.json
   ```

2. يمكن الآن عرض مفتاح الهوية العمومي (identity public key) عن طريق تشغيل:

   ```bash
     solana-keygen pubkey ~/validator-keypair.json
   ```

> ملاحظة: ملف "validator-keypair.json" هو أيضًا مفتاحك الخاص \(ed25519 \).

يُعرّف زوج مفاتيح هوية المُدقّق (validator identity keypair) الخاص بك بشكل فريد المُدقّق الخاص بك داخل الشبكة. من الأهمية بمكان إجراء نسخ إحتياطي لهذه المعلومات **It is crucial to back-up this information.**

إذا لم تقم بعمل نسخة إحتياطية من هذه المعلومات، فلن تكون قادرًا على إسترداد المُدقّق (validator) الخاص بك إذا فقدت الوصول إليها. إذا حدث هذا، فستفقد حِصَّتك من عملات SOL أيضًا.

لعمل نسخة إحتياطية من زوج مفاتيح الهوية (identity keypair) للمُدقّق (validator) الخاص بك، قُم بعمل نسخة إحتياطية من ملفك في مكان آمن **back-up your "validator-keypair.json” file to a secure location.**

## قُم بربط المفتاح العمومي (pubkey) في Solana بحساب قاعدة المفتاح (Keybase)

يجب عليك ربط المفتاح العمومي (Pubkey) في Solana بحساب Keybase.io. تصف الإرشادات التالية كيفية القيام بذلك عن طريق تثبيت Keybase على الخادم الخاص بك.

1. قُم بتثبيت [Keybase](https://keybase.io/download) على جهازك.
2. قُم بتسجيل الدخول إلى حساب Keybase الخاص بك على الخادم الخاص بك. أنشئ حساب Keybase أولاً إذا لم يكن لديك حساب بالفعل. هنا قائمة أوامر Keybase CLI الأساسية [list of basic Keybase CLI commands](https://keybase.io/docs/command_line/basics).
3. قُم بإنشاء دليل Solana في مُجلد الملف العام الخاص بك: `mkdir /keybase/public/<KEYBASE_USERNAME>/solana`
4. قُم بنشر زوج مفاتيح هوية المُدقّق (validator identity keypair) الخاص بك عن طريق إنشاء ملف فارغ في مُجلد ملف Keybase العام بالتنسيق التالي: `/keybase/public/<KEYBASE_USERNAME>/solana/validator-<BASE58_PUBKEY>`. على سبيل المثال:

   ```bash
     touch /keybase/public/<KEYBASE_USERNAME>/solana/validator-<BASE58_PUBKEY>
   ```

5. للتحقق من نشر المفتاح العمومي (public key)، تأكد من أنه يُمكنك التصفح بنجاح إلى `https://keybase.pub/<KEYBASE_USERNAME>/solana/validator-<BASE58_PUBKEY>`
