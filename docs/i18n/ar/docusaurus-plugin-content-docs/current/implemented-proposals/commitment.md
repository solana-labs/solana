---
title: الإلتزام (Commitment)
---

يهدف مقياس الإلتزام إلى منح العملاء مقياسًا لتأكيد الشبكة ومستويات الحِصَّة (stake) في كتلة (block) معينة. يمكن للعملاء بعد ذلك إستخدام هذه المعلومات لإشتقاق تدابير الإلتزام الخاصة بهم.

# حساب الـ RPC

يمكن للعملاء طلب مقاييس الإلتزام من المُدقّق `s` through `get_block_commitment(s: Signature) -> BlockCommitment` عبر الـ RPC. يحتوي الهيكل `BlockCommitment` على مصفوفة من u64 بأمر `[u64, MAX_CONFIRMATIONS]`. تمثل هذه المصفوفة مقياس الإلتزام للكتلة (block) المحددة `N` التي تحتوي على التوقيع `s` إعتبارًا من الكتلة `M` الأخيرة التي صوت المُدقّق (validator) عليها.

يُشير الإدخال `s` في الفهرس `i` في المصفوفة `BlockCommitment` إلى أن المُدقّق لاحظ `s` أن إجمالي الحِصَّة في الفُتحة التي تصل إلى `i` تأكيدات على الكتلة `N` كما لوحظ في بعض الكتل `M`. ستكون هناك عناصر `MAX_CONFIRMATIONS` في هذه المصفوفة، والتي تمثل جميع عدد التأكيدات الممكنة من 1 إلى `MAX_CONFIRMATIONS`.

# حساب مقياس الإلتزام (Computation of commitment metric)

يعمل بناء هيكل إلتزام الكُتلة `BlockCommitment` هذا على تعزيز العمليات الحسابية التي تم إجراؤها بالفعل للتوصل إلى إجماع (consensus). تعمل الدالة `collect_vote_lockouts` في `consensus.rs` على إنشاء خريطة التجزئة (HashMap)، حيث يكون كل إدخال على شكل `(b, s)` حيث يمثل `s` مقدار الحِصَّة (stake) في بنك `b`.

يتم إجراء هذا الحساب على بنك مرشح قابل للتصويت `b` على النحو التالي.

```text
   let output: HashMap<b, Stake> = HashMap::new();
   for vote_account in b.vote_accounts {
       for v in vote_account.vote_stack {
           for a in ancestors(v) {
               f(*output.get_mut(a), vote_account, v);
           }
       }
   }
```

حيث `f` هي بعض وظائف التراكم التي تُعدل إدخال الحِصَّة `Stake` للفُتحة `a` مع بعض البيانات المُشتقِّة من التصويت `v` و حساب التصويت `vote_account` (الحِصَّة، التأمين، إلخ). لاحظ هنا أن `ancestors` هنا يتضمن فقط الفُتحات (Slots) الموجودة في ذاكرة التخزين المؤقت للحالة الحالية. لن تكون التواقيع الخاصة بالبنوك التي تسبق تلك الموجودة في ذاكرة التخزين المُؤقت للحالة قابلة للإستعلام بأي حال من الأحوال، لذلك لم يتم تضمين هذه البنوك في حسابات الإلتزام هنا.

يمكننا الآن زيادة الحساب أعلاه بشكل طبيعي لبناء مصفوفة `BlockCommitment` لكل بنك `b` عن طريق:

1. إضافة `ForkCommitmentCache` لتجميع هيكل `BlockCommitment`
2. إستبدال `f` بـ `f'` بحيث يؤدي الحساب أعلاه أيضًا إلى إنشاء `BlockCommitment` لكل بنك `b`.

سننتقل إلى تفاصيل 2) لأن 1) أمر عديم الأهمية.

قبل المتابعة، من الجدير بالذكر أنه بالنسبة لحساب تصويت بعض المُدقّقين `a`، فإن عدد التأكيدات المحلية لهذا المُدقّق في الفُتحة `s` هو `v.num_confirmations`، حيث `v` هو أصغر تصويت في مجموعة الأصوات `a.votes` بحيث يكون `v.slot >= s` (أي ليست هناك حاجة للنظر في أي تصويت > v لأن عدد التأكيدات سيكون أقل).

الآن بشكل أكثر تحديدًا، نقوم بزيادة الحساب أعلاه إلى:

```text
   let output: HashMap<b, Stake> = HashMap::new();
   let fork_commitment_cache = ForkCommitmentCache::default();
   for vote_account in b.vote_accounts {
       // vote stack is sorted from oldest vote to newest vote
       for (v1, v2) in vote_account.vote_stack.windows(2) {
           for a in ancestors(v1).difference(ancestors(v2)) {
               f'(*output.get_mut(a), *fork_commitment_cache.get_mut(a), vote_account, v);
           }
       }
   }
```

حيث تم تعريف `f'` على النحو التالي:

```text
    fn f`(
        stake: &mut Stake,
        some_ancestor: &mut BlockCommitment,
        vote_account: VoteAccount,
        v: Vote, total_stake: u64
    ){
        f(stake, vote_account, v);
        *some_ancestor.commitment[v.num_confirmations] += vote_account.stake;
    }
```
