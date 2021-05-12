---
title: محفظة Ledger Nano S ومحفظة Ledger Nano X
---

يصف هذا النص كيفية إعداد محفظة [Ledger Nano S](https://shop.ledger.com/products/ledger-nano-s) أو محفظة [Ledger Nano X](https://shop.ledger.com/pages/ledger-nano-x) بواسطة برنامج [Ledger Live](https://www.ledger.com/ledger-live).

بمجرد الانتهاء من خطوات الإعداد التالي وتثبيت تطبيق Solana على محفظة Nano، سيكون لدى المستخدمين خيارات عدّة لكيفية استخدام جهاز النانو للتفاعل مع شبكة Solana [use the Nano to interact with the Solana Network](#interact-with-the-solana-network)

## لنبدأ

- قم بطلب [Nano S](https://shop.ledger.com/products/ledger-nano-s) أو [Nano X](https://shop.ledger.com/pages/ledger-nano-x) من Ledger.
- اتبع إرشادات أعداد الجهاز الموجودة في الحزمة أو من خلال صفحة البدء في Ledger [Ledger's Start page](https://www.ledger.com/start/)
- قم بتثبيت برنامج سطح المكتب [Ledger Live desktop software](https://www.ledger.com/ledger-live/)
  - إذا كان برنامج Ledger Live مثبتًا بالفعل، فقم بتحديثه إلى آخر نسخة مما يجعل آخر نسخة للبرنامج الثابت وتحديثات التطبيق متاحًا.
- قم بتوصيل محفظة Nano بالكمبيوتر الخاص بك واتبع التعليمات على الشاشة.
- قم بتحديث البرنامج الثابت على محفظة Nano الجديدة الخاصة بك. هذا ضروري للتأكد من أنك قادر على تثبيت آخر نسخة من تطبيق Solana.
  - [تحديث البرنامج الثابت لمحفظة Nano S](https://support.ledger.com/hc/en-us/articles/360002731113-Update-Ledger-Nano-S-firmware)
  - [تحديث البرنامج الثابت لمحفظة Nano X](https://support.ledger.com/hc/en-us/articles/360013349800)

## تثبيت تطبيق Solana على محفظة Nano الخاصة بك

- قم بتشغيل Ledger Live
- انقر فوق زر "مدير" على اللوحة اليسرى في التطبيق وقم بالبحث عن "Solana" في كتالوج التطبيق، ثم اضغط "تثبيت".
  - تأكد من توصيل جهازك بواسطة USB وغير مقفل برمز PIN
- قد يتم مطالبتك على محفظة Nano لتأكيد تثبيت تطبيق Solana
- ينبغي أن يظهر تطبيق "Solana" بأنه "تم تثبيته" في مدير Ledger Live

## ترقية تطبيق Solana إلى آخر نسخة

للتأكد من أنك تمتلك أحدث الخصائص في حال كنت تستخدم النسخة القديمة من تطبيق Solana، قم بالترقية من فضلك إلى نسخة `v1.0.1` من خلال اتباع الخطوات التالية.

- تأكد من أنك تمتلك نسخة 2.10.0 أو أحدث من برنامج Ledger Live.
  - للتحقق من نسخة برنامج Ledger Live، اضغط على زر الإعدادات في أعلى الزاوية اليمنى، ثم اضغط "حول". إذا كانت هناك نسخة أحدث من برنامج Ledger Live، ينبغي أن يظهر لك علامة تطلب منك التحديث عندما تقوم بفتح برنامج Ledger Live.
- قم بتحديث البرنامج الثابت على محفظة Nano الخاصة بك
  - [تحديث البرنامج الثابت لمحفظة Nano S](https://support.ledger.com/hc/en-us/articles/360002731113-Update-Ledger-Nano-S-firmware)
  - [تحديث البرنامج الثابت لمحفظة Nano X](https://support.ledger.com/hc/en-us/articles/360013349800)
- بعد نجاح عملية تحديث البرنامج الثابت، سترى أن تطبيق Solana تمت إعادة تثبيت النسخة الأحدث للتطبيق تلقائيًا.

## التفاعل مع شبكة Solana

يمكن للمستخدمين استخدام أي من الخيارات التالية لاستخدام محفظة Nano الخاصة بهم للتفاعل مع Solana:

- [SolFlare.com](https://solflare.com/) هي عبارة عن محفظة ويب غير محتضنة، وقد تم إنشاؤها خصيصًا لـ Solana وتدعم التحويلات الأساسية وعمليات التحصيص بواسطة جهاز محفظة Ledger. قم بقراءة الدليل الخاص بنا حول [using a Nano with SolFlare](solflare.md).

- يمكن للمطورين والمستخدمين المتقدمين استخدام جهاز Nano بواسطة أدوات سطر الأوامر [use a Nano with the Solana command line tools](hardware-wallets/ledger.md). الميزات الجديدة للمحفظة مدعومة دائما في أدوات سطر الأوامر الأصلية قبل أن تكون مدعومة من قبل محافظ الطرف الثالث.

## مشاكل معروفة

- أحيانا لا يمكن لمحفظة Nano X الاتصال بمحافظ الويب التي تستخدم نظام التشغيل Windows. ومن المحتمل أن يؤثر هذا على أي محافظ تعتمد على المتصفح من خلال استخدام WebUSB. ويعمل فريق دفتر الأستاذ على حل هذه المشكلة.

## الدعم

قم بزيارة صفحة الدعم الخاصة بالمحفظة [Wallet Support Page](support.md) لمعرفة طرق الحصول على مساعدة.
