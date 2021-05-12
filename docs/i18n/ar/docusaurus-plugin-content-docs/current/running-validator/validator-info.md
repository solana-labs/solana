---
title: نشر معلومات المُدقّق (Publishing Validator Info)
---

يُمكنك نشر معلومات المُدقّق (validator) الخاصة بك على الشبكة لتكون مرئية للعامة للمُستخدمين الآخرين.

## تشغيل solana validator-info

قُم بتشغيل solana CLI لملء حساب معلومات المُدقّق (validator):

```bash
solana validator-info publish --keypair ~/validator-keypair.json <VALIDATOR_INFO_ARGS> <VALIDATOR_NAME>
```

للحصول على تفاصيل حول الحقول الإختيارية لـ VALIDATOR_INFO_ARGS:

```bash
solana validator-info publish --help
```

## مثال على الأوامر

مثال على نشر الأمر (command):

```bash
solana validator-info publish "Elvis Validator" -n elvis -w "https://elvis-validates.com"
```

مثال على أمر الإستعلام (query command):

```bash
solana validator-info get
```

التي تُنتج

```text
Validator info from 8WdJvDz6obhADdxpGCiJKZsDYwTLNEDFizayqziDc9ah
  Validator pubkey: 6dMH3u76qZ7XG4bVboVRnBHR2FfrxEqTTTyj4xmyDMWo
  Info: {"keybaseUsername":"elvis","name":"Elvis Validator","website":"https://elvis-validates.com"}
```

## قاعدة المفتاح (Keybase)

يسمح تضمين إسم مُستخدم Keybase لتطبيقات العميل \ (مثل Solana Network Explorer \) بسحب ملف التعريف العام الخاص بالمُدقّق (validator) تلقائيًا، بما في ذلك أدلة التشفير وهوية العلامة التجارية وما إلى ذلك. لتوصيل المفتاح العمومي (pubkey) للمُدقّق الخاص بك (validator) مع الـ Keybase:

1. إنضم إلى [https://keybase.io/](https://keybase.io/) وأكمل الملف التعريفي للمُدقّق (validator) الخاص بك
2. قُم إضافة مفتاح هوية **identity pubkey** المُدقّق (validator) الخاص بك إلى الـ Keybase:

   - قُم بإنشاء ملف فارغ على جهاز الكمبيوتر المحلي الخاص بك يُسمى `validator-<PUBKEY>`
   - في الـ Keybase، إنتقل إلى قسم الملفات، وقم بتحميل ملف المفتاح العمومي (pubkey) الخاص بك إلى

     دليل فرعي `solana` في مجلدك العمومي `/keybase/public/<KEYBASE_USERNAME>/solana`

   - للتحقق من المفتاح العمومي (pubkey) الخاص بك، تأكد من أنه يُمكنك التصفح بنجاح إلى

     `https://keybase.pub/<KEYBASE_USERNAME>/solana/validator-<PUBKEY>`

3. قم بإضافة أو تحديث `solana validator-info` الخاص بك بإستخدام إسم مُستخدم الـ Keybase الخاص بك. ستتحقق

   أداة الـ CLI من ملف `validator-<PUBKEY>`
