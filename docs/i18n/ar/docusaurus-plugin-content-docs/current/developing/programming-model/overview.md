---
title: "نظرة عامة"
---

يتفاعل التطبيق [app](terminology.md#app) مع مجموعة Solana بإرسالها المُعاملات [transactions](transactions.md) مع واحد أو أكثر من تعليمة [instructions](transactions.md#instructions). يُمرر وقت التشغيل [runtime](runtime.md) الخاص بـ Solana مُسبقًا هذه التعليمات إلى البرامج [programs](terminology.md#program) التي تم نشرها بواسطة مُطوري التطبيقات. قد تُخبر التعليمات، على سبيل المثال، البرنامج بنقل [lamports](terminology.md#lamports) من الحساب [account](accounts.md) إلى آخر أو إنشاء عقد تفاعلي يحكم كيفية نقل الـ lamports. يتم تنفيذ التعليمات بشكل مُتسلسل وذري لكل مُعاملة. إذا كانت أي تعليمات غير صالحة، يتم تجاهل جميع تغييرات الحساب في المُعاملة.

لبدء التطوير على الفور يُمكنك بناء ونشر وتشغيل واحد من الأمثلة [examples](developing/deployed-programs/examples.md).
