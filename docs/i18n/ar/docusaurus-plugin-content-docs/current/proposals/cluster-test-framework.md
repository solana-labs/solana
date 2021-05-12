---
title: إطار إختبار المجموعات (Cluster Test Framework)
---

يقترح هذا المُستند إطار عمل إختبار الكتلة \(CTF\). الـ CTF عبارة عن أداة إختبار تسمح بإجراء الإختبارات على مجموعة (cluster) محلية قيد التشغيل أو مجموعة مُنتشرة.

## الحافز (Motivation)

الهدف من CTF هو توفير إطار عمل لكتابة الإختبارات بشكل مُستقل عن مكان وكيفية نشر الكتلة (cluster). يُمكن تسجيل الإنحدار في هذه الإختبارات ويُمكن تشغيل الإختبارات ضد المجموعات (clusters) المنشورة للتحقق من النشر. يجب أن يكون تركيز هذه الإختبارات على إستقرار المجموعة (cluster)، الإجماع (consensus)، التسامح مع الخطأ، إستقرار واجهة برمجة التطبيقات (API).

يجب أن تتحقق الإختبارات من خطأ أو سيناريو واحد، ويجب كتابتها بأقل قدر من السباكة الداخلية المُعرضة للإختبار.

## نظرة عامة على التصميم (Design Overview)

يتم توفير الاختبارات كنقطة دخول، وهي عبارة عن هيكل `contact_info::ContactInfo`، وزوج مفاتيح (keypair) تم شحن رصيده بالفعل.

تم تكوين كل عُقدة (node) في المجموعة (cluster) بإستخدام `validator::ValidatorConfig` في وقت الإشتغال أو التمهيد. في وقت الإشتغال أو التمهيد يُحدد هذا الإعداد أي إعداد للمجموعة الإضافية المطلوبة للإختبار. يجب أن تقوم المجموعة (cluster) بالتمهيد بالإشتغال (boot) عند تشغيلها في العملية أو في مركز البيانات.

بمجرد بدء التشغيل، سيكتشف الإختبار المجموعة (cluster) من خلال نقطة دخول القيل والقال (gossip) وتكوين أي سلوكيات وقت تشغيل عبر مُدقّق (validator) الـ RPC.

## إختبار الواجهة (Test Interface)

يبدأ كل إختبار CTF بنقطة دخول غير شفافة وزوج مفاتيح (keypair) مُمول. يجب ألا يعتمد الإختبار على كيفية نشر المجموعة (cluster)، ويجب أن يكون قادرًا على مُمارسة جميع وظائف المجموعة من خلال الواجهات المُتاحة للجمهور.

```text
use crate::contact_info::ContactInfo;
use solana_sdk::signature::{Keypair, Signer};
pub fn test_this_behavior(
    entry_point_info: &ContactInfo,
    funding_keypair: &Keypair,
    num_nodes: usize,
)
```

## إكتشاف المجموعات (Cluster Discovery)

في بداية الإختبار، تم إنشاء المجموعة (cluster) بالفعل وهي مُتصلة بالكامل. يمكن للإختبار إكتشاف مُعظم العُقد (nodes) المُتاحة خلال بضع ثوانٍ.

```text
use crate::gossip_service::discover_nodes;

// Discover the cluster over a few seconds.
let cluster_nodes = discover_nodes(&entry_point_info, num_nodes);
```

## إعدادات المجموعة (Cluster Configuration)

لتمكين سيناريوهات مُحددة، يجب تمهيد المجموعة (cluster) بإعدادات خاصة. يمكن إلتقاط هذه الإعدادات في `validator::ValidatorConfig`.

على سبيل المثال:

```text
let mut validator_config = ValidatorConfig::default();
let local = LocalCluster::new_with_config(
                num_nodes,
                10_000,
                100,
                &validator_config
                );
```

## كيفية تصميم إختبار جديد (How to design a new test)

على سبيل المثال، هناك خطأ يُظهر فشل المجموعة (cluster) عندما تغمرها عُقد (nodes) القيل والقال (gossip) مُعلن عنها أنها غير صالحة. قد تتغير مكتبة القيل والقال (gossip) والبروتوكول الخاص بنا، لكن المجموعة (cluster) لا تزال بحاجة إلى البقاء مرنة في مواجهة إغراقات عُقد (nodes) القيل والقال (gossip) المُعلن عنها غير الصالحة.

إعدادات خدمة الـ RPC أو Configure the RPC service:

```text
let mut validator_config = ValidatorConfig::default();
validator_config.rpc_config.enable_rpc_gossip_push = true;
validator_config.rpc_config.enable_rpc_gossip_refresh_active_set = true;
```

قم بتوصيل RPCs وأُكتب إختبارًا جديدًا:

```text
pub fn test_large_invalid_gossip_nodes(
    entry_point_info: &ContactInfo,
    funding_keypair: &Keypair,
    num_nodes: usize,
) {
    let cluster = discover_nodes(&entry_point_info, num_nodes);

    // Poison the cluster.
    let client = create_client(entry_point_info.client_facing_addr(), VALIDATOR_PORT_RANGE);
    for _ in 0..(num_nodes * 100) {
        client.gossip_push(
            cluster_info::invalid_contact_info()
        );
    }
    sleep(Durration::from_millis(1000));

    // Force refresh of the active set.
    for node in &cluster {
        let client = create_client(node.client_facing_addr(), VALIDATOR_PORT_RANGE);
        client.gossip_refresh_active_set();
    }

    // Verify that spends still work.
    verify_spends(&cluster);
}
```
