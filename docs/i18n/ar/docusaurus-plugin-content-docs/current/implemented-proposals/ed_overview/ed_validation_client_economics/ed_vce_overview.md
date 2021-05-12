---
title: إقتصاديات عُملاء المُصادقة (Validation-client Economics)
---

**قابل للتغيير. تابع أحدث المُناقشات الإقتصادية في مُنتديات Solana من الرابط التالي: https://forums.solana.com**

يحق لعُملاء المُصادقة (validation-clients) فرض عمولة على المكافآت التضخمية المُوزعة على الرموز التي يتم تحْصِيصها. هذا التعويض مُخصص لتوفير موارد الحوسبة \ (CPU + GPU \) للتحقق من صحة حالة مُعينة من حالة PoH والتصويت عليها. يتم تحديد هذه المكافآت القائمة على البروتوكول من خلال جدول خوارزمي للتضخم كدالة لإجمالي المعروض من العملات. من المُتوقع إطلاق الشبكة بمعدل تضخم سنوي يبلغ حوالي 8٪، ومن المُقرر أن تنخفض بنسبة 15٪ سنويًا حتى يتم الوصول إلى مُعدل ثابت طويل الأجل يبلغ 1.5٪، ومع ذلك لم يتم الإنتهاء من هذه المعايير من قبل المجتمع. سيتم تقسيم هذه الإصدارات وتوزيعها على المُدققين (validators) المشاركين، مع تخصيص حوالي 95٪ من الرموز لمكافآت المُدقق (validator) الأولية (5٪ المتبقية محجوزة لمصاريف تشغيل المؤسسة). Because the network will be distributing a fixed amount of inflation rewards across the stake-weighted validator set, the yield observed for staked tokens will be primarily a function of the amount of staked tokens in relation to the total token supply.

بالإضافة إلى ذلك، قد يكسب عُملاء المُصادقة (validation clients) إيرادات من خلال الرسوم عبر مُعاملات التثبت من صحة حالة المُصادقة (state-validation). من أجل الوضوح، نصف بشكل مُنفصل تصميم ودوافع توزيعات الإيرادات هذه لعُملاء المُصادقة (validation-clients) أدناه: التثبت من حالة المُصادقة (state-validation)، المُكافآت القائمة على البروتوكول (protocol-based rewards) ورسوم مُعاملات التثبت من حالة المُصادقة (state-validation) والإيجار.
