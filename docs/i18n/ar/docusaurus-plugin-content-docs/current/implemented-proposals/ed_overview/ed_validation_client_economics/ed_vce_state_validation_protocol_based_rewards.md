---
title: جدول التضخم (Inflation Schedule)
---

**قابل للتغيير. تابع أحدث المُناقشات الإقتصادية في مُنتديات Solana من الرابط التالي: https://forums.solana.com**

لدى عُملاء المُصادقة (validation-clients) دورين وظيفيين في شبكة Solana:

- التدقيق (Validate) من صحة \ (صوت \) الحالة العامة الحالية لـ PoH التي تمت مُلاحظتها.
- أن يتم إنتخابهم "كقائد" (leader) على جدول زمني مُرجح بالحصص (stake-weighted round-robin schedule) على مدار الساعة خلال الوقت الذي يكونون فيه مسؤولين عن جمع المُعاملات المُعلقة ودمجها في PoH المرصودة، وبالتالي تحديث الحالة العامة للشبكة وتوفير إستمرارية الشبكة.

يتم توزيع مكافآت عُملاء المُصادقة (validation-clients) لهذه الخدمات في نهاية كل فترة (epoch) في Solana. كما تمت مناقشته سابقًا، يتم تقديم تعويضات لعُملاء المُصادقة (validator-clients) من خلال عمولة يتم تحصيلها على مُعدل التضخم السنوي القائم على البروتوكول المُوزع بما يتناسب مع وزن الحِصَّة (stake) لكل عُقدة التدقيق (validator node) \ (أنظر أدناه \) جنبًا إلى جنب مع رسوم المُعاملات التي يُطالب بها القائد (leader) المُتاحة خلال كل دورة قائد (leader rotation). بمعنى آخر. خلال الوقت الذي يتم فيه إنتخاب عميل مُصادقة (validatior-client) مُعين كقائد (leader)، تكون لديه الفرصة للإحتفاظ بجزء من رسوم كل مُعاملة، ناقصًا المبلغ المُحدد بالبروتوكول الذي يتم إتلافه \ (راجع [Validation-client State Transaction Fees](ed_vce_state_validation_transaction_fees.md)\).

يجب أن تكون عائد التخزين السنوي الفعال المستند إلى البروتوكول \ (٪ \) لكل فترة (epoch) يتلقاها عُملاء المُصادقة (validatior-clients) أن تكون دالة لـ:

- مُعدل التضخم العام الحالي، المُشتق من جدول الإصدار المُضخم المُحدد مسبقًا \ (أُنظر [Validation-client Economics](ed_vce_overview.md)\)
- جزء من SOL المُحَصَّص (staked) من إجمالي العرض المُتداول الحالي،
- العمولة التي تتولاها خدمة التدقيق (validation)،
- وقت التشغيل / المشاركة \ [٪ من الفُتحات (Slots) المُتاحة التي أتيحت للمُدقق فرصة التصويت عليها \] لمُدقق مُعين خلال الفترة (epoch) السابقة.

العامل الأول هو وظيفة مُعلمات البروتوكول فقط \ (أي بغض النظر عن سلوك المُدقّق (validator) في فترة (epoch) مُعينة \) وينتج عنه جدول تضخم مُصمم لتحفيز المشاركة المُبكرة، وتوفيرإاستقرار نقدي واضح وتوفير الأمان الأمثل في الشبكة.

As a first step to understanding the impact of the _Inflation Schedule_ on the Solana economy, we’ve simulated the upper and lower ranges of what token issuance over time might look like given the current ranges of Inflation Schedule parameters under study.

تحديدا:

- _Initial Inflation Rate_: 7-9%
- _Dis-inflation Rate_: -14-16%
- _Long-term Inflation Rate_: 1-2%

بإستخدام هذه النطاقات لمُحاكاة عدد من جداول التضخم المُحتملة، يُمكننا إستكشاف التضخم بمرور الوقت:

![](/img/p_inflation_schedule_ranges_w_comments.png)

في الرسم البياني الوارد أعلاه، تُحدد القيم المُتوسطة للنطاق لتوضيح مُساهمة كل مُعلِّمة (parameter). From these simulated _Inflation Schedules_, we can also project ranges for token issuance over time.

![](/img/p_total_supply_ranges.png)

Finally we can estimate the _Staked Yield_ on staked SOL, if we introduce an additional parameter, previously discussed, _% of Staked SOL_:

%~\tنص{SOL Staked} = \frac{\tنص{Total SOL Staked}}{\tنص{Total Current Supply}}

In this case, because _% of Staked SOL_ is a parameter that must be estimated (unlike the _Inflation Schedule_ parameters), it is easier to use specific _Inflation Schedule_ parameters and explore a range of _% of Staked SOL_. على سبيل المثال أدناه، إخترنا مُنتصف نطاقات المُعلمة (parameter) المُستكشفة أعلاه:

- _Initial Inflation Rate_: 8%
- _Dis-inflation Rate_: -15%
- _Long-term Inflation Rate_: 1.5%

The values of _% of Staked SOL_ range from 60% - 90%, which we feel covers the likely range we expect to observe, based on feedback from the investor and validator communities as well as what is observed on comparable Proof-of-Stake protocols.

![](/img/p_ex_staked_yields.png)

Again, the above shows an example _Staked Yield_ that a staker might expect over time on the Solana network with the _Inflation Schedule_ as specified. This is an idealized _Staked Yield_ as it neglects validator uptime impact on rewards, validator commissions, potential yield throttling and potential slashing incidents. It additionally ignores that _% of Staked SOL_ is dynamic by design - the economic incentives set up by this _Inflation Schedule_.

### عائد إثبات الحِصَّة أو التحصيص المُعدّل (Adjusted Staking Yield)

A complete appraisal of earning potential from staking tokens should take into account staked _Token Dilution_ and its impact on staking yield. For this, we define _adjusted staking yield_ as the change in fractional token supply ownership of staked tokens due to the distribution of inflation issuance. بمعنى آخر. • الآثار المُخففة الإيجابية للتضخم.

We can examine the _adjusted staking yield_ as a function of the inflation rate and the percent of staked tokens on the network. يُمكننا أن نرى هذا المُخطط لكسور إثبات حِصَّة أو تحصيص (staking) مُختلفة هنا:

![](/img/p_ex_staked_dilution.png)
