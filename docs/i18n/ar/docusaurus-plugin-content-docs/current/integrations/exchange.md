---
title: إضافة Solana إلى منصة التداول (Exchange) الخاصة بك
---

يصف هذا الدليل كيفية إضافة رمز Solana المُسمى SOL إلى منصة تداول العملات المشفرة (cryptocurrency exchange) الخاصة بك.

## إعداد العُقدة (Node Setup)

نوصي بشدة بإعداد عُقدتين (nodes) على الأقل على أجهزة كمبيوتر عالية الجودة / مثيلات سحابية، والترقية إلى الإصدارات الأحدث على الفور، ومُراقبة عمليات الخدمة بإستخدام أداة مُراقبة مُجمعة.

يُمكّنك هذا الإعداد من:

- للحصول على بوابة موثوقة لمجموعة (cluster) الشبكة التجريبية الرئيسية (Mainnet Beta) الخاصة بـ Solana للحصول على البيانات وإرسال مُعاملات السحب
- للتحكم الكامل في مقدار بيانات تاريخ الكتلة (block) التي يتم الإحتفاظ بها
- للحفاظ على توافر الخدمة الخاصة بك حتى في حالة فشل عُقدة (node) واحدة

تتطلب العُقد (nodes) الخاصة بـ Solana قدرة حوسبة عالية نسبيًا للتعامل مع الكتل (blocks) السريعة ومُعاملات عالية في الثانية الواحدة (TPS). للحصول على مُتطلبات مُحددة، يُرجى الإطلاع على متطلبات الأجهزة المُوصى بها [hardware recommendations](../running-validator/validator-reqs.md).

لتشغيل عُقدة api:

1. [قُم بتثبيت مجموعة أدوات سطر الأوامر (command-line tool) الخاصة بـ Solana](../cli/install-solana-cli-tools.md)
2. قُم بتشغيل المُدقّق (validator) بالمُعلمات (parameters) التالية على الأقل:

```bash
solana-validator \
  --ledger <LEDGER_PATH> \
  --entrypoint <CLUSTER_ENTRYPOINT> \
  --expected-genesis-hash <EXPECTED_GENESIS_HASH> \
  --rpc-port 8899 \
  --no-voting \
  --enable-rpc-transaction-history \
  --limit-ledger-size \
  --trusted-validator <VALIDATOR_ADDRESS> \
  --no-untrusted-rpc
```

قُم بتخصيص `--ledger` لموقع تخزين دفتر الأستاذ (ledger) المطلوب، و `--rpc-port` إلى المنفذ (port) الذي تريد كشفه.

جميع مُعلمات `--entrypoint` و `--expected-genesis-hash` خاصة بالمجموعة (cluster) التي تنضم إليها. المُعلمات الحالية للشبكة التجريبية الرئيسية [Current parameters for Mainnet Beta](../clusters.md#example-solana-validator-command-line-2)

تسمح لك المُعلِّمة `--limit-ledger-size` بتحديد عدد قِطَع دفتر الأستاذ [shreds](../terminology.md#shred) الذي تحتفظ به العُقدة (node) على القرص. إذا لم تقم بتضمين هذه المُعلِّمة (parameter)، فسوف يحتفظ المُدقّق (validator) بدفتر الأستاذ (ledger) بالكامل حتى نفاد مساحة القرص. تُحاول القيمة الإفتراضية الإحتفاظ بإستخدام قرص دفتر الأستاذ (ledger) أقل من 500GB. قد يتم طلب إستخدام أكثر أو أقل للقرص عن طريق إضافة وسيطة إلى `--limit-ledger-size` إذا رغبت في ذلك. تحقق من `solana-validator --help` لقيمة الحد الإفتراضية المُستخدمة بواسطة `--limit-ledger-size`. مزيد من المعلومات حول إختيار قيمة حد مُخصصة متوفره هنا [available here](https://github.com/solana-labs/solana/blob/583cec922b6107e0f85c7e14cb5e642bc7dfb340/core/src/ledger_cleanup_service.rs#L15-L26).

يُمكن أن يُؤدي تحديد مُعلِّمة (parameter) أو أكثر من `--trusted-validator` إلى حمايتك من بدأ الإشتغال أو التمهيد (booting) من لقطة ضارة (malicious snapshot). المزيد عن قيمة التمهيد بإستخدام أدوات التحقق الموثوقة [More on the value of booting with trusted validators](../running-validator/validator-start.md#trusted-validators)

مُعلِّمات إختيارية يجب وضعها في الإعتبار (Optional parameters to consider):

- `--private-rpc` يمنع نشر منفذ (port) الـ RPC الخاص بك للإستخدام من قبل العُقَد (nodes) الأخرى
- يسمح لك `--rpc-bind-address` بتحديد عنوان IP مُختلف لربط منفذ (port) الـ RPC

### عمليات إعادة التشغيل التلقائية والمُراقبة (Automatic Restarts and Monitoring)

نوصي بتهيئة كل عُقدة (node) من عُقدك لإعادة التشغيل تلقائيًا عند الخروج، لضمان فقدان أقل قدر مُمكن من البيانات. يُعد تشغيل برنامج solana كخدمة systemd خيارًا رائعًا.

للمُراقبة، نقدم [`solana-watchtower`](https://github.com/solana-labs/solana/blob/master/watchtower/README.md)، والتي يُمكنها مُراقبة المُدقّق (validator) الخاص بك وإكتشاف إذا ما كانت عملية التدقيق `solana-validator` غير صِّحّية. يُمكن تهيئته مُباشرة لتنبيهك عبر Slack أو Telegram أو Discord أو Twilio. للحصول على التفاصيل، قُم ببتشغيل `solana-watchtower --help`.

```bash
solana-watchtower --validator-identity <YOUR VALIDATOR IDENTITY>
```

#### إعلانات إصدار برنامج جديد (New Software Release Announcements)

نقوم بإصدار برامج جديدة بشكل مُتكرر (حوالي إصدار واحد في الأسبوع). تتضمن الإصدارات الأحدث في بعض الأحيان تغييرات غير مُتوافقة في البروتوكول، مما يستلزم تحديث البرامج في الوقت المُناسب لتجنب الأخطاء في كتل (blocks) المُعالجة.

يتم إرسال إعلانات الإصدارات الرسمية لجميع أنواع الإصدارات (العادية والأمان) عبر قناة الـ discord التي تسمى [`#mb-announcement`](https://discord.com/channels/428295358100013066/669406841830244375) (`mb` stands for `mainnet-beta`).

مثل المُدقّقين المُحَصِّصين (staked validators)، نتوقع أن يتم تحديث أي مُدقّقين يتم التداول عليهم في أقرب وقت يُناسبك في غضون يوم عمل أو يومين بعد إعلان الإصدار العادي. بالنسبة للإصدارات المُتعلقة بالأمن، قد تكون هناك حاجة إلى إجراءات أكثر إلحاحًا.

### إستمرارية دفتر الأستاذ (Ledger Continuity)

بشكل إفتراضي، سيتم تشغيل كل عُقدة (nodes) من عُقدك من لقطة (snapshot) مُقدمة من أحد المُدقّقين (validators) الموثوق بهم. تعكس هذه اللقطة (snapshot) الحالة الحالية للشبكة، ولكنها لا تحتوي على تاريخ دفتر الأستاذ (ledger) الكامل. إذا خرجت إحدى عُقدك (node) وبدأت بالإشتغال أو التمهيد (boots) من لقطة (snapshot) جديدة، فقد تكون هناك فجوة في دفتر الأستاذ (ledger) في تلك العُقدة. لمنع هذه المُشكلة، أضف المُعلمة `--no-snapshot-fetch` إلى الأمر `solana-validator` لتلقي بيانات تاريخ دفتر الأستاذ (ledger) بدلاً من لقطة (snapshot).

لا تُمرر المُعلِّمة `- no-snapshot-fetch` أثناء بدأ الإشتغال أو التمهيد (boot) الأولي لأنه لا يمكن تمهيد العُقدة (node) بالكامل من كتلة التكوين (genesis block). بدلاً من ذلك، قُم بالتمهيد من لقطة (snapshot) أولاً ثم قم بإضافة المُعلِّمة `--no-snapshot-fetch` لإعادة التشغيل.

من المُهم مُلاحظة أن مقدار تاريخ دفتر الأستاذ المُتاح للعُقد (nodes) الخاصة بك من بقية الشبكة محدود في أي وقت. بمجرد التشغيل إذا واجه المُدقّقون (validators) وقت تعطل كبير، فقد لا يتمكنون من اللحاق بالشبكة وسيحتاجون إلى تنزيل لقطة (snapshot) جديدة من مُدقّق (validator) موثوق. عند القيام بذلك، سيكون لدى المُدقّقين (validators) لديك فجوة في بيانات تاريخ دفتر الأستاذ (ledger) الخاصة بهم والتي لا يُمكن ملؤها.

### تقليل كشف منفذ المُدقّق (Minimizing Validator Port Exposure)

يتطلب المُدقّق (validator) أن تكون منافذ (ports) الـ UDP و الـ TCP المُختلفة مفتوحة لحركة الزوار الواردة من جميع مُدقّقي Solana الآخرين. في حين أن هذا هو وضع التشغيل الأكثر فعالية، ويُوصى به بشدة، فمن المُمكن تقييد المُدقّق (validator) ليطلب فقط حركة الزوار الواردة من مُدقّق Solana آخر.

قم أولاً بإضافة الوسيطة `--restricted-repair-only-mode`. سيُؤدي ذلك إلى تشغيل المُدقّق (validator) في وضع مُقيد حيث لن يتلقى دفعات من بقية المُدقّقين، وبدلاً من ذلك سيحتاج إلى الإقتراع المُستمر لمُدقّقي التحقق من الكتل (blocks). سيقوم المُدقّق (validator) فقط بإرسال حزم (packets) الـ UDP إلى المُدقّقين الآخرين بإستخدام منافذ _Gossip_ و _ServeR_ ("خدمة الإصلاح") ، ولا يستقبل سوى حزم حزم (packets) الـ UDP على منفذي _Gossip_ و _Repair_.

منفذ _Gossip_ ثنائي الإتجاه ويسمح للمُدقّق (validator) الخاص بك بالبقاء على إتصال مع بقية المجموعة (cluster). يُرسل المُدقّق (validator) الخاص بك على _ServeR_ لتقديم طلبات إصلاح للحصول على كتل (blocks) جديدة من بقية الشبكة، حيث تم تعطيل التوربينة (Turbine) الآن. سيتلقى المُدقّق (validator) الخاص بك بعد ذلك إستجابات الإصلاح على المنفذ _Repair_ من المُدقّقين الآخرين.

لمزيد تقييد المُدقّق (validator) لطلب كتل (blocks) فقط من مُدقّق واحد أو أكثر، حدد أولاً مفتاح الهوية (identity pubkey) للمُدقّق وأضف 0000 وسيطة `--gossip-pull-validator PUBKEY --repair-validator PUBKEY` لمفتاح العمومي (PUBKEY). سيُؤدي ذلك إلى أن يكون المُدقّق (validator) الخاص بك بمثابة إستنزاف لموارد كل مُدقّق تضيفه، لذا يُرجى القيام بذلك بإعتدال وفقط بعد التشاور مع المُدقّق الهدف.

يجب أن يتصل المُدقّق (validator) الخاص بك الآن فقط مع المُدقّقين المُدرجين بشكل صريح وفقط على المنافذ (ports) التالية _Gossip_ و _Repair_ و منافذ _ServeR_.

## إعداد حسابات الإيداع (Setting up Deposit Accounts)

لا تتطلب حسابات Solana أي تهيئة على على الشبكة (on-chain)؛ بمجرد إحتوائها على بعض SOL، فهي موجودة. لإنشاء حساب إيداع لمنصة التداول (Exchange) الخاصة بك، ما عليك سوى إنشاء زوج مفاتيح (keypair) خاص بـ Solana بإستخدام أي من أدوات المحفظة [wallet tools](../wallet-guide/cli.md).

نوصي بإستخدام حساب إيداع فريد لكل مُستخدم لديك.

يتم فرض رسوم على حسابات Solana بقيمة الإيجار [rent](developing/programming-model/accounts.md#rent) عند الإنشاء ومرة واحدة لكل حساب ولكن يُمكن إعفاؤها من الإيجار إذا كانت تحتوي على قيمة إيجار عامين من عملات SOL. من أجل العثور على الحد الأدنى لرصيد الإعفاء من الإيجار لحسابات الودائع الخاصة بك، قُم بالإستعلام للحصول على الحد الأدنى من الرصيد للإعفاء من الإيجار [`getMinimumBalanceForRentExemption` endpoint](developing/clients/jsonrpc-api.md#getminimumbalanceforrentexemption):

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc": "2.0","id":1,"method":"getMinimumBalanceForRentExemption","params":[0]}' localhost:8899

{"jsonrpc":"2.0","result":890880,"id":1}
```

### الحسابات الغير مُتصلة (Offline Accounts)

قد ترغب في الإحتفاظ بالمفاتيح لحساب مجموعة واحد أو أكثر دون إتصال بالأنترنات لمزيد من الأمان. إذا كان الأمر كذلك، فستحتاج إلى نقل SOL إلى حسابات ساخنة بإستخدام [offline methods](../offline-signing.md).

## الإستماع إلى الإيداعات (Listening for Deposits)

عندما يُريد المُستخدم إيداع SOL في منصة التداول (Exchange) الخاصة بك، قُم بالطلب منهم بإرسال تحويل إلى عنوان الإيداع المُناسب.

### إستفتاء على الكتل (Poll for Blocks)

لتتبع جميع حسابات الإيداع الخاصة منصة التداول (Exchange) الخاصة بك، قُم بإجراء إستفتاء لكل كتلة (block) مؤكدة وفحص عناوين الإهتمام، بإستخدام خدمة JSON-RPC لـ API عُقدة Solana الخاصة بك.

- لتحديد الكتل (blocks) المُتاحة، أرسل [`getConfirmedBlocks` request](developing/clients/jsonrpc-api.md#getconfirmedblocks)، لتمرير آخر كتلة قُمت بمُعالجتها بالفعل كمُعلِّمة فُتحة البداية (start-slot):

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc": "2.0","id":1,"method":"getConfirmedBlocks","params":[5]}' localhost:8899

{"jsonrpc":"2.0","result":[5,6,8,9,11],"id":1}
```

لا تنتج كل فُتحة (Slot) كتلة (block)، لذلك قد تكون هناك فجوات في تسلسل الأعداد الصحيحة.

- لكل كتلة (block)، قُم بطلب مُحتوياتها بـ [`getConfirmedBlock` request](developing/clients/jsonrpc-api.md#getconfirmedblock):

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc": "2.0","id":1,"method":"getConfirmedBlock","params":[5, "json"]}' localhost:8899

{
  "jsonrpc": "2.0",
  "result": {
    "blockhash": "2WcrsKSVANoe6xQHKtCcqNdUpCQPQ3vb6QTgi1dcE2oL",
    "parentSlot": 4,
    "previousBlockhash": "7ZDoGW83nXgP14vnn9XhGSaGjbuLdLWkQAoUQ7pg6qDZ",
    "rewards": [],
    "transactions": [
      {
        "meta": {
          "err": null,
          "fee": 5000,
          "postBalances": [
            2033973061360,
            218099990000,
            42000000003
          ],
          "preBalances": [
            2044973066360,
            207099990000,
            42000000003
          ],
          "status": {
            "Ok": null
          }
        },
        "transaction": {
          "message": {
            "accountKeys": [
              "Bbqg1M4YVVfbhEzwA9SpC9FhsaG83YMTYoR4a8oTDLX",
              "47Sbuv6jL7CViK9F2NMW51aQGhfdpUu7WNvKyH645Rfi",
              "11111111111111111111111111111111"
            ],
            "header": {
              "numReadonlySignedAccounts": 0,
              "numReadonlyUnsignedAccounts": 1,
              "numRequiredSignatures": 1
            },
            "instructions": [
              {
                "accounts": [
                  0,
                  1
                ],
                "data": "3Bxs3zyH82bhpB8j",
                "programIdIndex": 2
              }
            ],
            "recentBlockhash": "7GytRgrWXncJWKhzovVoP9kjfLwoiuDb3cWjpXGnmxWh"
          },
          "signatures": [
            "dhjhJp2V2ybQGVfELWM1aZy98guVVsxRCB5KhNiXFjCBMK5KEyzV8smhkVvs3xwkAug31KnpzJpiNPtcD5bG1t6"
          ]
        }
      }
    ]
  },
  "id": 1
}
```

يسمح لك حقلا ما قبل الأرصدة `preBalances` و ما بعد الأرصدة `postBalances` بتعقب تغييرات الرصيد في كل حساب دون الحاجة إلى تحليل المُعاملة بالكامل. يسردون أرصدة البداية والنهاية لكل حساب في [lamports](../terminology.md#lamport)، مفهرسة إلى قائمة `accountKeys`. على سبيل المثال، إذا كان عنوان الإيداع إذا كانت الفائدة `47Sbuv6jL7CViK9F2NMW51aQGhfdpUu7WNvKyH645Rfi`، فإن هذه المُعاملة تُمثل تحويل 218099990000 - 207099990000 = 11000000000 lamports = 11 SOL

إذا كنت بحاجة إلى مزيد من المعلومات حول نوع المُعاملة أو تفاصيل أخرى، فيُمكنك طلب الكتلة (block) من الـ RPC بتنسيق ثنائي (binary format)، وتحليلها بإستخدام إما [Rust SDK](https://github.com/solana-labs/solana) أو [Javascript SDK](https://github.com/solana-labs/solana-web3.js).

### سِجِل العنوان (Address History)

يمكنك أيضًا الإستعلام عن سِجِل المُعاملات لعنوان مُعين. يُعد هذا بشكل عام طريقة غير _not_ قابلة للتطبيق لتتبع جميع عناوين الإيداع الخاصة بك عبر جميع الفُتحات (Slots)، ولكنها قد تكون مُفيدة لفحص عدد قليل من الحسابات لفترة زمنية مُحددة.

- أرسل طلب [`getConfirmedSignaturesForAddress2`](developing/clients/jsonrpc-api.md#getconfirmedsignaturesforaddress2) إلى واجهة برمجة تطبيقات (Api) العُقدة (node):

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc": "2.0","id":1,"method":"getConfirmedSignaturesForAddress2","params":["6H94zdiaYfRfPfKjYLjyr2VFBg6JHXygy84r3qhc3NsC", {"limit": 3}]}' localhost:8899

{
  "jsonrpc": "2.0",
  "result": [
    {
      "err": null,
      "memo": null,
      "signature": "35YGay1Lwjwgxe9zaH6APSHbt9gYQUCtBWTNL3aVwVGn9xTFw2fgds7qK5AL29mP63A9j3rh8KpN1TgSR62XCaby",
      "slot": 114
    },
    {
      "err": null,
      "memo": null,
      "signature": "4bJdGN8Tt2kLWZ3Fa1dpwPSEkXWWTSszPSf1rRVsCwNjxbbUdwTeiWtmi8soA26YmwnKD4aAxNp8ci1Gjpdv4gsr",
      "slot": 112
    },
    {
      "err": null,
      "memo": null,
      "signature": "dhjhJp2V2ybQGVfELWM1aZy98guVVsxRCB5KhNiXFjCBMK5KEyzV8smhkVvs3xwkAug31KnpzJpiNPtcD5bG1t6",
      "slot": 108
    }
  ],
  "id": 1
}
```

- لكل توقيع يتم إرجاعه، أحصل على تفاصيل المُعاملة بإرسال طلب الحُصول على مُعاملة مُؤكدة [`getConfirmedTransaction`](developing/clients/jsonrpc-api.md#getconfirmedtransaction) request:

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc": "2.0","id":1,"method":"getConfirmedTransaction","params":["dhjhJp2V2ybQGVfELWM1aZy98guVVsxRCB5KhNiXFjCBMK5KEyzV8smhkVvs3xwkAug31KnpzJpiNPtcD5bG1t6", "json"]}' localhost:8899

// Result
{
  "jsonrpc": "2.0",
  "result": {
    "slot": 5,
    "transaction": {
      "message": {
        "accountKeys": [
          "Bbqg1M4YVVfbhEzwA9SpC9FhsaG83YMTYoR4a8oTDLX",
          "47Sbuv6jL7CViK9F2NMW51aQGhfdpUu7WNvKyH645Rfi",
          "11111111111111111111111111111111"
        ],
        "header": {
          "numReadonlySignedAccounts": 0,
          "numReadonlyUnsignedAccounts": 1,
          "numRequiredSignatures": 1
        },
        "instructions": [
          {
            "accounts": [
              0,
              1
            ],
            "data": "3Bxs3zyH82bhpB8j",
            "programIdIndex": 2
          }
        ],
        "recentBlockhash": "7GytRgrWXncJWKhzovVoP9kjfLwoiuDb3cWjpXGnmxWh"
      },
      "signatures": [
        "dhjhJp2V2ybQGVfELWM1aZy98guVVsxRCB5KhNiXFjCBMK5KEyzV8smhkVvs3xwkAug31KnpzJpiNPtcD5bG1t6"
      ]
    },
    "meta": {
      "err": null,
      "fee": 5000,
      "postBalances": [
        2033973061360,
        218099990000,
        42000000003
      ],
      "preBalances": [
        2044973066360,
        207099990000,
        42000000003
      ],
      "status": {
        "Ok": null
      }
    }
  },
  "id": 1
}
```

## إرسال عمليات السحب (Sending Withdrawals)

لإستيعاب طلب المُستخدم سحب SOL، يجب عليك إنشاء مُعاملة تحويل Solana، وإرسالها إلى واجهة برمجة تطبيقات (Api) العُقدة (node) لإعادة توجيهها إلى مجموعتك.

### المُتزامن (Synchronous)

يُتيح لك إرسال نقل مُتزامن إلى مجموعة Solana ضمان نجاح عملية النقل وإنهائها بواسطة المجموعة (cluster) بسهولة.

تُوفر أداة سطر الأوامر (command-line tool) في Solana أمرًا بسيطًا، `solana transfer`، لإنشاء مُعاملات التحويل وإرسالها وتأكيدها. من المُفترض، ستنتظر هذه الطريقة وتتبع التقدم على stderr حتى يتم الإنتهاء من المُعاملة بواسطة المجموعة (cluster). إذا فشلت المُعاملة، فسوف تُبلغ عن أي أخطاء في المُعاملة.

```bash
solana transfer <USER_ADDRESS> <AMOUNT> --allow-unfunded-recipient --keypair <KEYPAIR> --url http://localhost:8899
```

يُقدم [Solana Javascript SDK](https://github.com/solana-labs/solana-web3.js) نهجًا مُشابهًا للنظام البيئي JS. إستخدم برنامج النظام `SystemProgram` لإنشاء مُعاملة تحويل وإرسالها بإستخدام طريقة إرسال وتأكيد المُعاملة `sendAndConfirmTransaction`.

### الغير مُتزامن (Asynchronous)

لمزيد من المُرونة، يُمكنك إرسال تحويلات السحب بشكل غير مُتزامن. في هذه الحالات، تقع على عاتقك مسؤولية التحقق من نجاح المُعاملة وإتمامها بواسطة المجموعة (cluster).

**Note:** تحتوي كل مُعاملة على تجزئة الكتلة الحديثة [recent blockhash](developing/programming-model/transactions.md#blockhash-format) تدل على حيويتها. من الأهمية بمكان **critical** الإنتظار حتى إنتهاء صلاحية تجزئة الكتلة (blockhash) قبل إعادة مُحاولة تحويل السحب الذي لا يبدو أنه تم تأكيده أو الإنتهاء منه بواسطة المجموعة (cluster). خلاف ذلك ، فإنك تُخاطر بإنفاق مُزدوج (double spend). شاهد المزيد على إنتهاء صلاحية تجزئة الكتلة [blockhash expiration](#blockhash-expiration) أدناه.

أُحصل أولاً على تجزئة الكتلة (blockhash) الحديثة بإستخدام أمر الحصول على الرسوم [`getFees` endpoint](developing/clients/jsonrpc-api.md#getfees) أو CLI:

```bash
solana fees --url http://localhost:8899
```

في أداة سطر الأوامر (command-line tool)، مرر الوسيطة `--no-wait` لإرسال تحويل بشكل غير مُتزامن، وقم بتضمين تجزئة الكتلة (blockhash) الحديثة مع الوسيطة `--blockhash`:

```bash
solana transfer <USER_ADDRESS> <AMOUNT> --no-wait --allow-unfunded-recipient --blockhash <RECENT_BLOCKHASH> --keypair <KEYPAIR> --url http://localhost:8899
```

يُمكنك أيضًا إنشاء المُعاملة وتوقيعها وتسلسلها يدويًا، وإطلاقها على المجموعة (cluster) بإستخدام JSON-RPC [`sendTransaction` endpoint](developing/clients/jsonrpc-api.md#sendtransaction).

#### تأكيدات المُعاملات & سرعة إثباتها (Transaction Confirmations & Finality)

أُحصل على حالة مجموعة المُعاملات بإستخدام [`getSignatureStatuses` JSON-RPC endpoint](developing/clients/jsonrpc-api.md#getsignaturestatuses). يُوضح حقل التأكيدات `confirmations` عدد الكتل المُؤكدة [confirmed blocks](../terminology.md#confirmed-block) التي إنقضت منذ مُعالجة المُعاملة. إذا كانت التأكيدات لاغية `confirmations: null`، فلقد تأكيدها [finalized](../terminology.md#finality).

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0", "id":1, "method":"getSignatureStatuses", "params":[["5VERv8NMvzbJMEkV8xnrLkEaWRtSz9CosKDYjCJjBRnbJLgp8uirBgmQpjKhoR4tjF3ZpRzrFmBV6UjKdiSZkQUW", "5j7s6NiJS3JAkvgkoc18WVAsiSaci2pxB2A6ueCJP4tprA2TFg9wSyTLeYouxPBJEMzJinENTkpA52YStRW5Dia7"]]}' http://localhost:8899

{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 82
    },
    "value": [
      {
        "slot": 72,
        "confirmations": 10,
        "err": null,
        "status": {
          "Ok": null
        }
      },
      {
        "slot": 48,
        "confirmations": null,
        "err": null,
        "status": {
          "Ok": null
        }
      }
    ]
  },
  "id": 1
}
```

#### إنتهاء صلاحية تجزئة الكتلة (Blockhash Expiration)

You can check whether a particular blockhash is still valid by sending a [`getFeeCalculatorForBlockhash`](developing/clients/jsonrpc-api.md#getfeecalculatorforblockhash) request with the blockhash as a parameter. If the response value is `null`, the blockhash is expired, and the withdrawal transaction using that blockhash should never succeed.

### التحقق من صحة عناوين الحساب التي يُوفرها المُستخدم لعمليات السحب

نظرًا لأن عمليات السحب لا يُمكن التراجع عنها، فقد يكون من المُمارسات الجيدة التحقق من صحة عنوان الحساب المُقدم من المُستخدم قبل الإذن بالسحب من أجل منع الخسارة العرضية لأموال المُستخدم.

#### Basic verfication

Solana addresses a 32-byte array, encoded with the bitcoin base58 alphabet. This results in an ASCII text string matching the following regular expression:

```
[1-9A-HJ-NP-Za-km-z]{32,44}
```

This check is insufficient on its own as Solana addresses are not checksummed, so typos cannot be detected. To further validate the user's input, the string can be decoded and the resulting byte array's length confirmed to be 32. However, there are some addresses that can decode to 32 bytes despite a typo such as a single missing character, reversed characters and ignored case

#### Advanced verification

Due to the vulnerability to typos described above, it is recommended that the balance be queried for candidate withdraw addresses and the user prompted to confirm their intentions if a non-zero balance is discovered.

#### Valid ed25519 pubkey check

عنوان الحساب العادي في Solana هو سلسلة مفتاح العمومي مُرمّزة Base58 من ed25519 256-bit. ليست كل أنماط الـ bit هي مفاتيح عامة صالحة لمنحنى ed25519، لذلك من المُمكن التأكد من أن عناوين الحساب التي يُوفرها المُستخدم هي المفاتيح العامة الصحيحة ed25519 على الأقل.

#### الجافا (Java)

فيما يلي مثال Java للتحقق من صحة العنوان المُقدم من المُستخدم كالمفتاح عمومي ed25519 صالح:

يفترض نموذج التعليمات البرمجية التالي أنك تستخدم Maven.

`pom.xml`:

```xml
<repositories>
  ...
  <repository>
    <id>spring</id>
    <url>https://repo.spring.io/libs-release/</url>
  </repository>
</repositories>

...

<dependencies>
  ...
  <dependency>
      <groupId>io.github.novacrypto</groupId>
      <artifactId>Base58</artifactId>
      <version>0.1.3</version>
  </dependency>
  <dependency>
      <groupId>cafe.cryptography</groupId>
      <artifactId>curve25519-elisabeth</artifactId>
      <version>0.1.0</version>
  </dependency>
<dependencies>
```

```java
import io.github.novacrypto.base58.Base58;
import cafe.cryptography.curve25519.CompressedEdwardsY;

public class PubkeyValidator
{
    public static boolean verifyPubkey(String userProvidedPubkey)
    {
        try {
            return _verifyPubkeyInternal(userProvidedPubkey);
        } catch (Exception e) {
            return false;
        }
    }

    public static boolean _verifyPubkeyInternal(String maybePubkey) throws Exception
    {
        byte[] bytes = Base58.base58Decode(maybePubkey);
        return !(new CompressedEdwardsY(bytes)).decompress().isSmallOrder();
    }
}
```

## دعم الرمز بصيغة SPL أو Supporting the SPL Token Standard

الرمز [SPL Token](https://spl.solana.com/token) هو المعيار لإنشاء الرموز المُغلفة / الإصطناعية وتبادلها في بلوكشاين Solana.

يُشبه سير عمل الرمز بصيغة SPL سير عمل رموز SOL الأصلية، ولكن هناك بعض الإختلافات التي ستتم مُناقشتها في هذا القسم.

### سَكّ الرموز (Token Mints)

Each _type_ of SPL Token is declared by creating a _mint_ account. يُخزن هذا الحساب البيانات الوصفية التي تصف ميزات الرمز مثل العرض وعدد الكسور العشرية والسلطات المُختلفة التي تتحكم في السك عملية (mint). يُشير كل حساب رمز بصيغة SPL إلى السَكّ (mint) المُرتبط به وقد يتفاعل فقط مع رموز بصيغة SPL من هذا النوع.

### تثبيت أداة `spl-token` الخاصة بـ CLI

يتم الإستعلام عن حسابات الرمز بصيغة SPL وتعديلها بإستخدام الأداة المُساعدة لسطر الأوامر `spl-token`. تعتمد الأمثلة الواردة في هذا القسم على تثبيتها على النظام المحلي.

يتم توزيع `spl-token` من الصندوق [crates.io](https://crates.io/crates/spl-token) الخاص بـ Rust للأداة المُساعدة لسطر أوامر `cargo` عبر حُمولة Rust. يُمكن تثبيت أحدث إصدار حُمولة من `cargo` بإستخدام بطانة واحدة سهلة الإستخدام لمنصتك على [rustup.rs](https://rustup.rs). بمجرد تثبيت حُمولتك `cargo`، يُمكن الحصول على `spl-token` بالأمر التالي:

```
cargo install spl-token-cli
```

يُمكنك بعد ذلك التحقق من الإصدار المُثبت للتحقق

```
spl-token --version
```

والتي يجب أن تُؤدي إلى شيء مثل

```text
spl-token-cli 2.0.1
```

### إنشاء حساب (Account Creation)

تحمل حسابات الرمز بصيغة SPL مُتطلبات إضافية لا تقوم بها حسابات برنامج النظام الأصلي:

1. يجب إنشاء حسابات الرمز بصيغة SPL قبل إيداع مبلغ من الرموز. يُمكن إنشاء حسابات رموز بشكل صريح بإستخدام الأمر إنشاء حساب الرمز `spl-token create-account`، أو ضمنيًا بواسطة الأمر `spl-token transfer --fund-recipient ...`.
1. يجب أن تظل الحسابات مُعفاة من الإيجار [rent-exempt](developing/programming-model/accounts.md#rent-exemption) طوال مُدة وجودها، وبالتالي تتطلب إيداع مبلغ صغير من رموز SOL الأصلية عند إنشاء الحساب. بالنسبة لحسابات الرمز بصيغة SPL النسخة 2، يكون هذا المبلغ 0.00203928 (2,039,280 lamports) من عملة SOL.

#### سطر الأوامر (Command Line)

لإنشاء حساب الرمز بصيغة SPL بالخصائص التالية:

1. المُرتبطة بالسك المُقدم (Associated with the given mint)
1. مملوكة من قبل زوج المفاتيح (keypair) لحساب التمويل

```
spl-token create-account <TOKEN_MINT_ADDRESS>
```

#### مثال

```
$ spl-token create-account AkUFCWTXb3w9nY2n6SFJvBV6VwvFUCe4KBMCcgLsa2ir
Creating account 6VzWGL51jLebvnDifvcuEDec17sK6Wupi4gYhm5RzfkV
Signature: 4JsqZEPra2eDTHtHpB4FMWSfk3UgcCVmkKkP7zESZeMrKmFFkDkNd91pKP3vPVVZZPiu5XxyJwS73Vi5WsZL88D7
```

أو لإنشاء حساب الرمز بصيغة SPL مع زوج مفاتيح (keypair) مُعين:

```
$ solana-keygen new -o token-account.json
$ spl-token create-account AkUFCWTXb3w9nY2n6SFJvBV6VwvFUCe4KBMCcgLsa2ir token-account.json
Creating account 6VzWGL51jLebvnDifvcuEDec17sK6Wupi4gYhm5RzfkV
Signature: 4JsqZEPra2eDTHtHpB4FMWSfk3UgcCVmkKkP7zESZeMrKmFFkDkNd91pKP3vPVVZZPiu5XxyJwS73Vi5WsZL88D7
```

### التحقق من رصيد الحساب (Checking an Account's Balance)

#### سطر الأوامر (Command Line)

```
spl-token balance <TOKEN_ACCOUNT_ADDRESS>
```

#### مثال

```
$ solana balance 6VzWGL51jLebvnDifvcuEDec17sK6Wupi4gYhm5RzfkV
0
```

### تحويل الرموز (Token Transfers)

الحساب المُصدر لعملية التحويل هو حساب الرمز الفعلي الذي يحتوي على المبلغ.

مع ذلك، يُمكن أن يكون عنوان المُستلم حساب محفظة عادي. في حالة عدم وجود حساب رمز (token account) مُرتبط بسك العملة المُحدد لهذه المحفظة بعد، فسيقوم التحويل بإنشائه بشرط أن تكون الوسيطة `--fund-Receient` على النحو المنصوص عليه.

#### سطر الأوامر (Command Line)

```
spl-token transfer <SENDER_ACCOUNT_ADDRESS> <AMOUNT> <RECIPIENT_WALLET_ADDRESS> --fund-recipient
```

#### مثال (Example)

```
$ spl-token transfer 6B199xxzw3PkAm25hGJpjj3Wj3WNYNHzDAnt1tEqg5BN 1 6VzWGL51jLebvnDifvcuEDec17sK6Wupi4gYhm5RzfkV
Transfer 1 tokens
  Sender: 6B199xxzw3PkAm25hGJpjj3Wj3WNYNHzDAnt1tEqg5BN
  Recipient: 6VzWGL51jLebvnDifvcuEDec17sK6Wupi4gYhm5RzfkV
Signature: 3R6tsog17QM8KfzbcbdP4aoMfwgo6hBggJDVy7dZPVmH2xbCWjEj31JKD53NzMrf25ChFjY7Uv2dfCDq4mGFFyAj
```

### الإيداع (Depositing)

نظرًا لأن كل زوج `(user, mint)` يتطلب حسابًا مُنفصلاً عن الشبكة، فمن المُستحسن أن تقوم منصة التداول (Exchange) بإنشاء مجموعات من حسابات الرمز مُقدمًا وتعيينها للمُستخدمين عند الطلب. يجب أن تكون جميع هذه الحسابات مملوكة من قبل منصة التداول (Exchange) التي تُسيطر عليها أزواج المفاتيح (keypairs).

يجب أن تتبع مُراقبة مُعاملات الإيداع طريقة الإقتراع على الكتلة [block polling](#poll-for-blocks) المُوضحة أعلاه. يجب فحص كل كتلة جديدة بحثًا عن المُعاملات الناجحة التي تُنتج الرمز بصيغة SPL بطلب تحويل [Transfer](https://github.com/solana-labs/solana-program-library/blob/096d3d4da51a8f63db5160b126ebc56b26346fc8/token/program/src/instruction.rs#L92) أو تحويل [Transfer2](https://github.com/solana-labs/solana-program-library/blob/096d3d4da51a8f63db5160b126ebc56b26346fc8/token/program/src/instruction.rs#L252) تُشير إلى حسابات المُستخدمين، ثم الإستعلام عن تحديثات رصيد حساب الرمز [token account balance](developing/clients/jsonrpc-api.md#gettokenaccountbalance).

يتم إجراء الإعتبارات [Considerations](https://github.com/solana-labs/solana/issues/12318) لتوسيع حقلي البيانات الوصفية لحالة المُعاملة ما قبل الرصيد `preBalance` وما بعد الرصيد `postBalance` لتشمل تحويلات رصيد حساب الرمز بصيغة SPL.

### السحب (Withdrawal)

يجب أن يكون عنوان السحب الذي يُوفره المُستخدم هو نفس العنوان المُستخدم لسحب عملات SOL العادية.

قبل تنفيذ عملية سحب [transfer](#token-transfers)، يجب على منصة التداول (Exchange) التحقق من العنوان كما هو موضح أعلاه [described above](#validating-user-supplied-account-addresses-for-withdrawals).

من عنوان السحب، تم تحديد حساب الرمز المُرتبط بالسك (mint) الصحيح والتحويل الصادر إلى هذا الحساب. لاحظ أنه من المُحتمل أن حساب الرمز المُرتبط لم يكن موجودًا بعد، وعند هذه النقطة يجب أن تُمول البورصة الحساب نيابة عن المُستخدم. بالنسبة لحساب الرمز بصيغة SPL النسخة 2، سيتطلب تمويل حساب السحب المبلغ 0.00203928 (2،039،280 lamports) من SOL عملة.

أمر نموذج إرسال رمز `spl-token transfer` للسحب:

```
$ spl-token transfer --fund-recipient <exchange token account> <withdrawal amount> <withdrawal address>
```

### إعتبارات أخرى (Other Considerations)

#### سلطة التجميد (Freeze Authority)

For regulatory compliance reasons, an SPL Token issuing entity may optionally choose to hold "Freeze Authority" over all accounts created in association with its mint. هذا يسمح لهم بتجميد [freeze](https://spl.solana.com/token#freezing-accounts) الأصول في حساب مُعين حسب الرغبة، مما يجعل الحساب غير قابل للإستخدام حتى يتم فك تجميده. إذا كانت هذه الميزة قيد الإستخدام، فسيتم تسجيل المفتاح العمومي (pubkey) التابع لسلطة التجميد في حساب الرمز بصيغة SPL الخاص بالـ mint.

## إختبار التكامل (Testing the Integration)

تأكد من إختبار سير العمل الكامل الخاص بك على شبكتي Solana التجريبية (testnet) وشبكة المُطورين (Devnet) بـ [clusters](../clusters.md) قبل الإنتقال إلى الإنتاج على الشبكة التجريبية الرئيسية (Mainnet Beta). شبكة المُطورين (Devnet) هي الأكثر إنفتاحًا ومرونة، وهي مثالية للتطوير الأولي، بينما توفر الشبكة التجريبية (testnet) إعدادات أكثر واقعية للمجموعة (cluster). Both devnet and testnet support a faucet, run `solana airdrop 1` to obtain some devnet or testnet SOL for developement and testing.
