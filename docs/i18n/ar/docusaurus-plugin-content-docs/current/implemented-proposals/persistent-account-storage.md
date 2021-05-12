---
title: التخزين المُستمر للحساب (Persistent Account Storage)
---

## التخزين المُستمر للحساب (Persistent Account Storage)

The set of accounts represent the current computed state of all the transactions that have been processed by a validator. كل المُدقّق (validator) يحتاج إلى الحفاظ على هذه المجموعة بأكملها. Each block that is proposed by the network represents a change to this set, and since each block is a potential rollback point, the changes need to be reversible.

التخزين الدائم مثل NVMEs أرخص 20 إلى 40 مرة من DDR. The problem with persistent storage is that write and read performance is much slower than DDR. Care must be taken in how data is read or written to. Both reads and writes can be split between multiple storage drives and accessed in parallel. This design proposes a data structure that allows for concurrent reads and concurrent writes of storage. Writes are optimized by using an AppendVec data structure, which allows a single writer to append while allowing access to many concurrent readers. The accounts index maintains a pointer to a spot where the account was appended to every fork, thus removing the need for explicit checkpointing of state.

## إلحاق النواقل (AppendVec)

إلحاق النواقل (AppendVec) هو هيكل بيانات يسمح بالقراءات العشوائية المُتزامنة مع كاتب إلحاق واحد فقط. تتطلب زيادة سعة إلحاق النواقل (AppendVec) أو تغيير حجمها وصولاً خاصًا. يتم تنفيذ ذلك بإزاحة ذرية `offset`، والتي يتم تحديثها في نهاية مُلحق مُكتمل.

الذاكرة الأساسية لإلحاق النواقل (AppendVec) هي ملف ذاكرة مُعين (memory-mapped file). تسمح ملفات الذاكرة المُعينة (memory-mapped files) بالوصول العشوائي السريع ويتم التعامل مع الترحيل بواسطة نظام التشغيل.

## فهرس الحساب (Account Index)

تم تصميم فهرس الحساب لدعم فهرس واحد لجميع الحسابات المُتشعبة حاليًا.

```text
type AppendVecId = usize;

type Fork = u64;

struct AccountMap(Hashmap<Fork, (AppendVecId, u64)>);

type AccountIndex = HashMap<Pubkey, AccountMap>;
```

الفهرس عبارة عن خريطة حساب مفتاتيح عمومية (pubkeys) لخريطة الإنقسامات أو الشوكات (Forks) وموقع بيانات الحساب في إلحاق النواقل (AppendVec). للحصول على نسخة حساب لإنقسام أو شوكة (fork):

```text
/// Load the account for the pubkey.
/// This function will load the account from the specified fork, falling back to the fork's parents
/// * fork - a virtual Accounts instance, keyed by Fork.  Accounts keep track of their parents with Forks,
///       the persistent store
/// * pubkey - The Account's public key.
pub fn load_slow(&self, id: Fork, pubkey: &Pubkey) -> Option<&Account>
```

تتم القراءة عن طريق الإشارة إلى موقع مُعين للذاكرة في `AppendVecId` عند الإزاحة المُخزنة. يُمكن إرجاع المرجع بدون نسخة.

### إنقسامات أو شوكات الجذر (Root Forks)

يُحدد [Tower BFT](tower-bft.md) في النهاية إنقسام أو شوكة (fork) كشوكة جذر (root fork) ويتم سحق الإنقسام أو الشوكة. لا يمكن التراجع عن الإنقسام أو الشوكة الجذر / المسحوقة.

عندما يتم سحق الإنقسام أو الشوكة (fork)، يتم سحب جميع الحسابات الموجودة في العناصر الرئيسية غير الموجودة بالفعل في الإنقسام أو الشوكة (fork) إلى أعلى نقطة عن طريق تحديث الفهارس. الحسابات ذات الرصيد الصفري في الإنقسام أو الشوكة (fork) المسحوقة تتم إزالتها من الإنقسام أو الشوكة (fork) عن طريق تحديث الفهارس.

يُمكن أن يتم جمع الحساب الغير مرغوب فيه _garbage-collected_ عندما يجعله السحق غير قابل للوصول.

هناك ثلاثة خيارات مُمكنة:

- الحفاظ على مجموعة تجزئة (HashSet) من الإنقسامات أو الشوكات الجذرية (root forks). من المُتوقع أن يتم إنشاء واحد كل ثانية. الشجرة (tree) بأكملها يُمكن جمعها في وقت لاحق. بدلاً من ذلك، إذا إحتفظت كل إنقسام أو شوكة (fork) بعدد مرجعي للحسابات، فقد تحدث عملية جمع البيانات المُهملة في أي وقت يتم فيه تحديث موقع الفهرس.
- إزالة أي إنقسامات أو شوكات (forks) منبوذة من الفهرس. يُمكن إعتبار أي إنقسامات أو شوكات (forks) مُتبقية أقل في العدد من الجذر على أنها جذر.
- مسح الفهرس، نقل أي جذور قديمة إلى جذور جديدة. أي إنقسام أو شوكة (fork) مُتبقية أقل من الجذر الجديد يُمكن حذفها لاحقاً.

## Garbage collection

As accounts get updated, they move to the end of the AppendVec. Once capacity has run out, a new AppendVec can be created and updates can be stored there. Eventually references to an older AppendVec will disappear because all the accounts have been updated, and the old AppendVec can be deleted.

To speed up this process, it's possible to move Accounts that have not been recently updated to the front of a new AppendVec. This form of garbage collection can be done without requiring exclusive locks to any of the data structures except for the index update.

The initial implementation for garbage collection is that once all the accounts in an AppendVec become stale versions, it gets reused. The accounts are not updated or moved around once appended.

## Index Recovery

Each bank thread has exclusive access to the accounts during append, since the accounts locks cannot be released until the data is committed. But there is no explicit order of writes between the separate AppendVec files. To create an ordering, the index maintains an atomic write version counter. Each append to the AppendVec records the index write version number for that append in the entry for the Account in the AppendVec.

To recover the index, all the AppendVec files can be read in any order, and the latest write version for every fork should be stored in the index.

## Snapshots

To snapshot, the underlying memory-mapped files in the AppendVec need to be flushed to disk. The index can be written out to disk as well.

## Performance

- الكتابات المُلحقة فقط (Append-only Writes) سريعة. تسمح مُحركات الأقراص SSDs و NVME، بالإضافة إلى جميع هياكل بيانات kernel على مُستوى نظام التشغيل، بتشغيل المُلحقات بالسرعة التي يسمح بها عرض النطاق الترددي (bandwidth) لـ PCI أو NVMe \(2,700 MB/الثانية\).
- كل إعادة تشغيل وعملية جزئية للبنك (bank thread) تُكتب بشكل مُتزامن إلى إلحاق النواقل (AppendVec) الخاص به.
- يُمكن إستضافة كل إلحاق النواقل (AppendVec) على NVMe مُنفصل.
- كل إعادة تشغيل وعملية جزئية للبنك (banking thread) لها وصول قراءة مُتزامن لجميع مُلحقات النواقل (AppendVecs) دون حظر عمليات الكتابة.
- يتطلب الفهرس قفل كتابة خاص بالكتابة. أداء عملية جزئية مُفردة (Single-thread) لتحديثات خريطة التجزئة (HashMap) هو في حدود 10 أمتار في الثانية الواحدة.
- يجب أن تستخدم المراحل المصرفية وإعادة التشغيل 32 عملية جزئية (threads) لكل NVMe. تتمتع NVMes بأداء مثالي مع 32 قارئًا أو كاتبًا مُتزامنًا.
