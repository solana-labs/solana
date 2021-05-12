---
title: Transaction Fees
---

**قابل للتغيير.**

Each transaction sent through the network, to be processed by the current leader validation-client and confirmed as a global state transaction, contains a transaction fee. تقدم رسوم المُعاملات العديد من المزايا في تصميم Solana الإقتصادي، على سبيل المثال:

- توفير تعويض الوحدة لشبكة المُدقّق (validator) لموارد وحدة المُعالجة المركزية (CPU) / وحدة مُعالجة الرسومات (GPU) اللازمة لمُعالجة مُعاملة الحالة (state transaction)،
- تقليل البريد العشوائي على الشبكة من خلال إدخال التكلفة الحقيقية للمُعاملات،
- توفير إستقرار إقتصادي مُحتمل طويل الأجل للشبكة من خلال الحد الأدنى لمبلغ الرسوم لكل مُعاملة الذي تم الحصول عليه من خلال البروتوكول، كما هو مُوضح أدناه.

Network consensus votes are sent as normal system transfers, which means that validators pay transaction fees to participate in consensus.

تعتمد العديد من إقتصادات البلوكشاين الحالية (مثل Bitcoin و Ethereum \) على المُكافآت المُستندة على البروتوكول لدعم الإقتصاد على المدى القصير، مع إفتراض أن الإيرادات المُتولدة من رسوم المُعاملات ستدعم الإقتصاد على المدى الطويل، عندما تنتهي المُكافآت المُشتقة من البروتوكول. In an attempt to create a sustainable economy through protocol-based rewards and transaction fees, a fixed portion (initially 50%) of each transaction fee is destroyed, with the remaining fee going to the current leader processing the transaction. يوفر مُعدل التضخم العالمي المُجدول مصدرًا للمُكافآت المُوزعة على عُملاء المُصادقة (validation-clients)، من خلال العملية المُوضحة أعلاه.

يتم تحديد رسوم المُعاملات بواسطة مجموعة الشبكة بناءً على مُعدل تاريخ النقل الأخير، أنظر إلى الرسوم المدفوعة بالإزدحام [Congestion Driven Fees](implemented-proposals/transaction-fees.md#congestion-driven-fees). This minimum portion of each transaction fee can be dynamically adjusted depending on historical _signatures-per-slot_. بهذه الطريقة، يمكن للبروتوكول إستخدام الحد الأدنى من الرسوم لإستهداف إستخدام الأجهازة المطلوب. By monitoring a protocol specified _signatures-per-slot_ with respect to a desired, target usage amount, the minimum fee can be raised/lowered which should, in turn, lower/raise the actual _signature-per-slot_ per block until it reaches the target amount. يمكن إعتبار عملية التعديل هذه مُشابهة لخوارزمية تعديل الصعوبة في بروتوكول Bitcoin، ولكن في هذه الحالة يتم تعديل الحد الأدنى لرسوم المُعاملة لتوجيه إستخدام أجهزة مُعالجة المُعاملات إلى المستوى المطلوب.

كما ذكرنا، يجب التخلص من نسبة ثابتة من رسوم كل مُعاملة. The intent of this design is to retain leader incentive to include as many transactions as possible within the leader-slot time, while providing an inflation limiting mechanism that protects against "tax evasion" attacks \(i.e. side-channel fee payments\).

بالإضافة إلى ذلك، يمكن أن تكون الرسوم المُحترقة أحد الإعتبارات في إختيار الإنقسام أو الشوكة (fork). في حالة وجود مُفترق PoH مع قائد (leader) ضار، خاضع للرقابة، نتوقع أن يكون إجمالي الرسوم التي تم إتلافها أقل من إنقسام أو شوكة (fork) صادقة مُماثلة، بسبب الرسوم المفقودة من الرقابة. إذا كان قائد (leader) الرقابة سيعوض عن رسوم البروتوكول المفقودة، فسيتعين عليه إستبدال الرسوم المحروقة في إنقسامهم أو شوكتهم (fork)، وبالتالي من المُحتمل أن يقلل من الحافز للرقابة في المقام الأول.
