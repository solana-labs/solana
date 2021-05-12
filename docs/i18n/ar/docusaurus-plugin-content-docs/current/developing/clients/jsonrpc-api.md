---
title: واجهة برمجة تطبيقات الـ JSON RPC أو JSON RPC API
---

تقبل العُقَد (nodes) في Solana طلبات HTTP باستخدام مُواصفات [JSON-RPC 2.0](https://www.jsonrpc.org/specification).

للتفاعل مع عُقدة (node) في Solana داخل تطبيق JavaScript، إستخدم مكتبة [solana-web3. s](https://github.com/solana-labs/solana-web3.js) التي تُعطي واجهة مُناسبة لطُرق RPC.

## نقطة نهاية HTTP RPC أو RPC HTTP Endpoint

المَنفَذ المُفترض **Default port:** 8899 eg. [http://localhost:8899](http://localhost:8899), [http://192.168.1.88:8899](http://192.168.1.88:8899)

## نُقطة نهاية RPC PubSub WebSocket أو RPC PubSub WebSocket Endpoint

**Default port:** 8900 eg. ws://localhost:8900, [http://192.168.1.88:8900](http://192.168.1.88:8900)

## الطُرق (Methods)

- [الحصول على معلومات الحساب (getAccountInfo)](jsonrpc-api.md#getaccountinfo)
- [الحصول على الرصيد (getBalance)](jsonrpc-api.md#getbalance)
- [الحصول على إلتزام الكتلة (getBlockCommitment)](jsonrpc-api.md#getblockcommitment)
- [الحصول على وقت الكتلة (getBlockTime)](jsonrpc-api.md#getblocktime)
- [الحصول على عُقَد المجموعة (getClusterNodes)](jsonrpc-api.md#getclusternodes)
- [الحصول على الكتلة المُؤَكدة (getConfirmedBlock)](jsonrpc-api.md#getconfirmedblock)
- [الحصول على الكتل المُؤَكدة (getConfirmedBlocks)](jsonrpc-api.md#getconfirmedblocks)
- [الحصول على الكتل المُؤَكدة بحدود (getConfirmedBlocksWithLimit)](jsonrpc-api.md#getconfirmedblockswithlimit)
- [الحصول على التوقيعات المُؤَكدة للعنوان (getConfirmedSignaturesForAddress)](jsonrpc-api.md#getconfirmedsignaturesforaddress)
- [الحصول على التوقيعات المُؤَكدة للعنوان 2 (getConfirmedSignaturesForAddress2)](jsonrpc-api.md#getconfirmedsignaturesforaddress2)
- [الحصول على المُعاملة المُؤَكدة (getConfirmedTransaction)](jsonrpc-api.md#getconfirmedtransaction)
- [الحصول على معلومات الفترة (getEpochInfo)](jsonrpc-api.md#getepochinfo)
- [الحصول على جدول الفترة (getEpochSchedule)](jsonrpc-api.md#getepochschedule)
- [الحصول على حاسبة الرسوم لتجزئة الكتلة (getFeeCalculatorForBlockhash)](jsonrpc-api.md#getfeecalculatorforblockhash)
- [الحصول على مُحافظ معدل الرسوم (getFeeRateGovernor)](jsonrpc-api.md#getfeerategovernor)
- [الحصول على الرسوم (getFees)](jsonrpc-api.md#getfees)
- [الحصول على أول كتلة مُتاحة (getFirstAvailableBlock)](jsonrpc-api.md#getfirstavailableblock)
- [الحصول على تجزئة مرحلة التكوين (getGenesisHash)](jsonrpc-api.md#getgenesishash)
- [الحصول على الصِّحَّة (getHealth)](jsonrpc-api.md#gethealth)
- [الحصول على الهوية (getIdentity)](jsonrpc-api.md#getidentity)
- [الحصول على مُحافظ التَضَخُّم (getInflationGovernor)](jsonrpc-api.md#getinflationgovernor)
- [الحصول على مُعدل التَضَخُّم (getInflationRate)](jsonrpc-api.md#getinflationrate)
- [الحصول على أكبر الحسابات (getLargestAccounts)](jsonrpc-api.md#getlargestaccounts)
- [الحصول على جدول القائد (getLeaderSchedule)](jsonrpc-api.md#getleaderschedule)
- [الحصول على الحد الأدنى للرصيد للإعفاء من الإيجار (getMinimumBalanceForRentExemption)](jsonrpc-api.md#getminimumbalanceforrentexemption)
- [الحصول على حسابات مُتعددة (getMultipleAccounts)](jsonrpc-api.md#getmultipleaccounts)
- [الحصول على حسابات البرنامج (getProgramAccounts)](jsonrpc-api.md#getprogramaccounts)
- [الحصول على تجزئة الكتلة الحديثة (getRecentBlockhash)](jsonrpc-api.md#getrecentblockhash)
- [الحصول على نماذج الأداء الحديثة (getRecentPerformanceSamples)](jsonrpc-api.md#getrecentperformancesamples)
- [الحصول على حالات التوقيع (getSignatureStatuses)](jsonrpc-api.md#getsignaturestatuses)
- [الحصول على الفُتحة (getSlot)](jsonrpc-api.md#getslot)
- [الحصول على قائد الفُتحة (getSlotLeader)](jsonrpc-api.md#getslotleader)
- [الحصول على تنشيط الحِصَّة (getStakeActivation)](jsonrpc-api.md#getstakeactivation)
- [الحصول على المعروض (getSupply)](jsonrpc-api.md#getsupply)
- [الحصول على عدد المُعاملات (getTransactionCount)](jsonrpc-api.md#gettransactioncount)
- [الحصول على الإصدار (getVersion)](jsonrpc-api.md#getversion)
- [الحصول على حسابات التصويت (getVoteAccounts)](jsonrpc-api.md#getvoteaccounts)
- [الحصول على الحد الأدنى لفُتحة دفتر الأستاذ (minimumLedgerSlot)](jsonrpc-api.md#minimumledgerslot)
- [طلب التوزيع الحر (requestAirdrop)](jsonrpc-api.md#requestairdrop)
- [إرسال المُعاملة (sendTransaction)](jsonrpc-api.md#sendtransaction)
- [مُحاكاة المُعاملة (simulateTransaction)](jsonrpc-api.md#simulatetransaction)
- [تعيين عامل فلترة السجل (setLogFilter)](jsonrpc-api.md#setlogfilter)
- [خروج المُدقّق (validatorExit)](jsonrpc-api.md#validatorexit)
- [إشتراك الWebsocket أو Subscription Websocket](jsonrpc-api.md#subscription-websocket)
  - [الإشتراك في الحساب (accountSubscribe)](jsonrpc-api.md#accountsubscribe)
  - [إلغاء الإشتراك في الحساب (accountUnsubscribe)](jsonrpc-api.md#accountunsubscribe)
  - [سجلات الإشتراك (logsSubscribe)](jsonrpc-api.md#logssubscribe)
  - [سجلات إلغاء الإشتراك (logsUnsubscribe)](jsonrpc-api.md#logsunsubscribe)
  - [الإشتراك في البرنامج (programSubscribe)](jsonrpc-api.md#programsubscribe)
  - [إلغاء الإشتركك في البرنامج (programSubscribe)](jsonrpc-api.md#programunsubscribe)
  - [الإشتراك في التوقيع (signatureSubscribe)](jsonrpc-api.md#signaturesubscribe)
  - [إلغاء الإشتراك في التوقيع (signatureSubscribe)](jsonrpc-api.md#signatureunsubscribe)
  - [الإشتراكك في الفُتحة (slotSubscribe)](jsonrpc-api.md#slotsubscribe)
  - [إلغاء الإشتراك في الفُتحة (slotUnsubscribe)](jsonrpc-api.md#slotunsubscribe)

## الأساليب غير المُستقرة (Unstable Methods)

قد تشهد الطرق غير المُستقرة (Unstable methods) تغيرات في إطلاقات التصحيح (patch) وقد لا تكون مدعومة إلى الأبد.

- [الحصول على رصيد حِساب الرموز (getTokenAccountBalance)](jsonrpc-api.md#gettokenaccountbalance)
- [الحصول على حِسابات الرموز حسب التفويض (getTokenAccountsByDelegate)](jsonrpc-api.md#gettokenaccountsbydelegate)
- [الحصول على حِسابات الرموز حسب المالك (getTokenAccountsByOwner)](jsonrpc-api.md#gettokenaccountsbyowner)
- [الحصول على أكبر حِسابات الرموز (getTokenLargestAccounts)](jsonrpc-api.md#gettokenlargestaccounts)
- [الحصول على معروض الرمز (getTokenSupply)](jsonrpc-api.md#gettokensupply)

## طلب التنسيق (Request Formatting)

لتقديم طلب JSON-RPC، أرسل طلب HTTP POST مع رأس `Content-Type: application/json`. يجب أن تحتوي بيانات طلب JSON على 4 حقول:

- `jsonrpc: <string>`، مُعيّن إلى `"2.0"`
- المُعرف `id: <number>`، العدد الصحيح الفريد من نوعه تم إنشاؤه من قبل العميل
- الطريقة `: <string>`، سلسلة تحتوي على الطريقة التي سيتم إستدعاؤها
- المُعطيات: `params: <array>`، مجموعة JSON من قِيَم المُعلِّمة (parameter) المطلوبة

مثال بإستخدام حليقة (curl):

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getBalance",
    "params": [
      "83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri"
    ]
  }
'
```

سيكون ناتج الإستجابة (response output) كائن JSON مع الحقول التالية:

- `jsonrpc: <string>`، مُطابق لمُواصفات الطلب
- المُعرف `id: <number>`، مُطابق لمُعرف الطلب
- النتيجة `result: <array|number|object|string>`، طلب البيانات أو تأكيد النجاح

يمكن إرسال الطلبات على دفعات عن طريق إرسال مصفوفة (array) من كائنات طلب JSON-RPC كبيانات لوظيفة واحدة.

## التعريفات (Definitions)

- التجزئة (Hash): تجزئة SHA-256 من قطعة من البيانات.
- المفتاح العمومي (pubkey): المفتاح العمومي لزوج مفاتيح Ed25519.
- المُعاملة: قائمة بتعليمات Solana مُوقّعة من زوج مفاتيح (keypair) العميل للإذن بتلك الإجراءات.
- التوقيع (Signature): توقيع Ed25519 على بيانات الحمولة الخاصة بالمُعاملة، بما في ذلك التعليمات. يمكن إستخدام ذلك لتحديد المُعاملات.

## إعداد إلتزام الحالة (Configuring State Commitment)

فيما يتعلق بعمليات الإختبار المبدئي (preflight) ومُعالجة المُعاملات، تختار عُقَد Solana البنك الذي يجب الإستعلام عنه بناء على شرط الإلتزام الذي حدده العميل. يصف الإلتزام كيف يتم الإنتهاء من الكتلة (block) في ذلك الوقت. عند الإستعلام عن حالة دفتر الأستاذ (ledger) ، يُوصى بإستخدام مُستويات إلتزام أقل للإبلاغ عن التقدم ومُستويات أعلى لضمان عدم التراجع عن الحالة.

بالترتيب التنازلي للإلتزام (الأكثر إكتمالًا إلى الأقل إكتمالًا) ، يُمكن للعملاء تحديد:

- الحد الأقصى `"max"` - سوف تستفسر العُقدة (node) عن أحدث كتلة (block) أكدتها الأغلبية العظمى (supermajority) من المجموعة (cluster) على أنها وصلت إلى الحد الأقصى للقِفْل ، بمعنى أن المجموعة إعترفت بهذه الكتلة (block) بصيغتها النهائية
- الجذر `"root"` - سوف تستفسر العُقدة (node) عن أحدث كتلة (block) وصلت إلى قفل أقصى (maximum lockout) على هذه العُقدة (node)، بمعنى أن العُقدة (node) قد تعرفت على هذه الكتلة (block) بصيغتها النهائية
- `"singleGossip"` - سوف تستفسر العُقدة (node) عن أحدث كتلة (block) تم التصويت عليها من قبل الأغلبية العُظمى (supermajority) للمجموعة (cluster).
  - يتضمن أصوات القيل والقال (gossip) وإعادتها.
  - فهو لا يحسب الأصوات على سُلالة مُتحدِّري (descendants) الكتلة (block)، بل يُوجهون الأصوات على تلك الكتلة (block).
  - يدعم مُستوى التأكيد هذا أيضا ضمانات "التأكيد المُتفائل" (optimistic confirmation) في الإصدار 1.3 وما بعده.
- الأحدث `"recent"` - العُقدة (node) سوف تستفسر عن أحدث كتلة (block). لاحظ أن الكتلة (block) قد لا تكون كاملة.

لمُعالجة العديد من المُعاملات التابعة في سلاسل ما، يُوصى بإستخدام إلتزام `"singleGossip"` ، الذي يُوازن بين السرعة وأمان التراجع. للسلامة الكاملة، من المُستحسن إستخدام الإلتزام الأقصى`"max"`.

#### مثال

يجب إدراج مُعلِّمة (parameter) الإلتزام كآخر عنصر في مُعطيات `params` المصفوفة:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getBalance",
    "params": [
      "83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri",
      {
        "commitment": "max"
      }
    ]
  }
'
```

#### المُفتَرَض (Default):

إذا لم يتم توفير إعدادات الإلتزام، ستكون العُقدة (node) مُلتزمة بالإعداد الافتراضي الأقصى `"max"`

فقط الطرق التي تستفسر عن حالة البنك تقبل مُعلِّمة (parameter) الإلتزام. مُشار إليها في مرجع واجهة برمجة التطبيقات (API) الوارد أدناه.

#### هيكل إستجابة الـ Rpc أو RpcResponse Structure

العديد من الطرق التي تأخذ مُعلِّمة (parameter) إلتزام تُعيد كائن RpcResponse JSON مُكون من جزأين:

- السياق `context`: بنية RpcResponseContext JSON بما في ذلك حقل فُتحة `slot` حيث تم تقييم العملية.
- القيمة `value`: القيمة التي تم إرجاعها من قبل العملية نفسها.

## فحص الصِّحَّة (Health Check)

على الرغم من أنه ليس JSON RPC API، يُوفر `GET /Health` في نقطة نهاية RPC HTTP آلية فحص صِّحّي لإستخدامها من قبل مُوازن التحميل (load balancers) أو أي شبكة بنية تحتية أخرى. هذا الطلب سوف يُعيد دائماً رد HTTP 200 OK مع جسم من "ok" أو "behind" إستناداً إلى الشروط التالية:

1. إذا كان واحد أو أكثر من مُدقّق موثوق `--trusted-validator` يتم تقديم حجج إلى مُدقّق `solana-validator`، يتم إرجاع "ok" عندما تكون العُقدة (node) موجودة خلال `HEALTH_CHECK_SLOT_DISTANCE` فُتحات (slots) أعلى مُدقّق (validator) موثوق به، خلاف ذلك يتم إرجاع "behind".
2. رسالة "ok" تُعاد دائماً إذا لم يتم توفير أي مُدقّقين (validators) موثوقين.

## مرجع JSON RPC API

### الحصول على معلومات الحساب (getAccountInfo)

يُرجع جميع المعلومات المُرتبطة بالحساب المُقدم للمفتاح العمومي (pubkey)

#### المُعلمات (parameters):

- `<string>` - المفتاح العمومي (pubkey) للحساب الذي سيتم الإستفسار عنه، كسلسلة مُرمّزة base-58
- `<object>` - كائن تكوين (إختياري) يحتوي على الحقول الإختيارية التالية:
  - (إختياري) الإلتزام [Commitment](jsonrpc-api.md#configuring-state-commitment)
  - الترميز `encoding: <string>` - الترميز لبيانات الحساب، إما "base58" بطيء "(_slow_) ، "base64", "base64+zstd" ، أو "jsonParsed". يقتصر "base58" على بيانات الحساب التي تقل عن 128 bytes. سيقوم "base64" بإرجاع البيانات المُرمّزة لـ base64 لبيانات الحساب من أي حجم. "base64+zstd" يضغط على بيانات الحساب بإستخدام [Zstandard](https://facebook.github.io/zstd/) و base64-en النتيجة. يُحاول ترميز "jsonParsed" إستخدام مُوزعي الحالة الخاصين بالبرنامج لإرجاع المزيد من بيانات حالة الحساب التي يُمكن قراءتها بشكل واضح. إذا طُلب "jsonParsed" ولكن لا يُمكن العثور على مُحلل (parser)، يعود الحقل إلى الترميز "base64"، يُمكن الكشف عنها عندما تكون بيانات `data` الحقل هو النوع `<string>`.
  - (إختياري) شريحة البيانات `dataSlice: <object>` - الحد من بيانات الحساب التي تم إرجاعها بإستخدام المُوازنة المُقدمة `: <usize>` و طول `length: <usize>` الحقول؛ مُتاح فقط لترميز "base58", "base64" أو "base64+zstd".

#### النتائج:

ستكون النتيجة كائن RpcResponse JSON بقيمة `value` تُساوي:

- `<null>` - إذا كان الحساب المطلوب غير موجود
- `<object>` - خلاف ذلك، كائن JSON يحتوي على:
  - الـ `lamports: <u64>`، عدد الـ lamports المُخَصَّصَة لهذا الحساب، كـ u64
  - المالك `owner: <string>`، المفتاح العمومي (pubkey) المُرمّز base-58 من البرنامج الذي تم تعيين هذا الحساب له
  - البيانات `data: <[string, encoding]|object>`، البيانات المُرتبطة بالحساب، إما كبيانات ثنائية مُرمّزة (encoded binary data) أو بصيغة JSON `{<program>: <state>}`، إعتماد على مُعلِّمة (parameter) الترميز
  - `executable: <bool>`، المنطقية (boolean) تُشير إلى ما إذا كان الحِساب يحتوي على برنامج \(وهو للقراءة-فقط\)
  - `إيجار الفترة: <u64>`، الفترة (epoch) التي سيكون فيها هذا الحساب مدينا بالإيجار القادم، مثل u64

#### مثال:

الطلب:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getAccountInfo",
    "params": [
      "vines1vzrYbzLMRdu58ou5XTby4qAqVRLmqo36NKPTg",
      {
        "encoding": "base58"
      }
    ]
  }
'
```

الإستجابة:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1
    },
    "value": {
      "data": [
        "11116bv5nS2h3y12kD1yUKeMZvGcKLSjQgX6BeV7u1FrjeJcKfsHRTPuR3oZ1EioKtYGiYxpxMG5vpbZLsbcBYBEmZZcMKaSoGx9JZeAuWf",
        "base58"
      ],
      "executable": false,
      "lamports": 1000000000,
      "owner": "11111111111111111111111111111111",
      "rentEpoch": 2
    }
  },
  "id": 1
}
```

#### مثال:

الطلب:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getAccountInfo",
    "params": [
      "4fYNw3dojWmQ4dXtSGE9epjRGy9pFSx62YypT7avPYvA",
      {
        "encoding": "jsonParsed"
      }
    ]
  }
'
```

الإستجابة:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1
    },
    "value": {
      "data": {
        "nonce": {
          "initialized": {
            "authority": "Bbqg1M4YVVfbhEzwA9SpC9FhsaG83YMTYoR4a8oTDLX",
            "blockhash": "3xLP3jK6dVJwpeGeTDYTwdDK3TKchUf1gYYGHa4sF3XJ",
            "feeCalculator": {
              "lamportsPerSignature": 5000
            }
          }
        }
      },
      "executable": false,
      "lamports": 1000000000,
      "owner": "11111111111111111111111111111111",
      "rentEpoch": 2
    }
  },
  "id": 1
}
```

### الحصول على الرصيد (getBalance)

يُرجع رصيد حساب المُقدم للمفتاح العمومي (pubkey)

#### المُعلمات (parameters):

- `<string>` - المفتاح العمومي (pubkey) للحساب الذي سيتم الإستفسار عنه، كسلسلة مُرمّزة base-58
- `<object>` (إختياري) الإلتزام [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### النتائج:

- `RpcResponse<u64>` - RpcResponse JSON كائن مع القيمة `value` تعيين الحقل إلى الرصيد

#### مثال:

الطلب:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getBalance", "params":["83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri"]}
'
```

النتيجة:

```json
{
  "jsonrpc": "2.0",
  "result": { "context": { "slot": 1 }, "value": 0 },
  "id": 1
}
```

### الحصول على إلتزام الكتلة (getBlockCommitment)

يُرجع الإلتزام لكتلة (block) مُعينة

#### المُعلمات (parameters):

- `<u64>` - الكتلة (block)، التي حددتها الفُتحة (Slot)

#### النتائج:

سيكون حقل النتيجة كائن JSON يحتوي على:

- الإلتزام `commitment` - الإلتزام، الذي يشمل إما:
  - `<null>` - كتلة (block) مجهولة
  - `<array>` - إلتزام، مجموعة من الأعداد الصحيحة u64 تُسجل كمية حِصَة (stake) المجموعة (cluster) في الlamports التي صوتت على الكتلة (block) في كل عمق من 0 إلى `MAX_LOCOUT_HISTORY` + 1
- مجموع الحِصَة `totalStake` - إجمالي الحِصَة (stake) النشطة، في الـ lamports، من الفترة (epoch) الحالية

#### مثال:

الطلب:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getBlockCommitment","params":[5]}
'
```

النتيجة:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "commitment": [
      0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
      0, 0, 0, 0, 0, 10, 32
    ],
    "totalStake": 42
  },
  "id": 1
}
```

### الحصول على وقت الكتلة (getBlockTime)

يُرجع الوقت التقديري لإنتاج كتلة (block) مُؤكدة.

يقوم كل مُدقّق (validator) بالإبلاغ عن وقت UTC الخاص به إلى دفتر الأستاذ (ledger) بفترة زمنية مُنتظمة عن طريق إضافة ختم زمني (Timestamp) بشكل مُتقطع إلى تصويت كتلة (block) مُعينة. يتم حساب وقت الكتلة (block time) المطلوبة من المُرَجَّح بالحِصَّة (stake-weighted) لتصويت الأختام الزمنية (Timestamps) في مجموعة من الكتل (blocks) الحديثة المُسجلة في دفتر الأستاذ (ledger).

ستُعيد العُقدة (node) التي تم بتشغيلها من لقطة (snapshot) أو تحديد حجم دفتر الأستاذ (ledger) (عن طريق إزالة الفُتحات القديمة) أختام زمنية (Timestamps) فارغة للكتل (blocks) تحت أدنى جذر لها +`TIMESTAMP_SLOT_RANGE`. يجب على المُستخدمين المهتمين بالحصول على هذه البيانات التاريخية الإستعلام عن عُقدة (node) مبنية من مرحلة التكوين (Genesis) والإحتفاظ بدفتر الأستاذ (ledger) بأكمله.

#### المُعلمات (parameters):

- `<u64>` - الكتلة (block)، التي حددتها الفُتحة (Slot)

#### النتائج:

- `<i64>` - الوقت التقديري للإنتاج، كختم زمني Unx (ثوان منذ الفترة Unix)
- `<null>` - الختم الزمني (Timestamp) غير متوفر لهذه الكتلة (block)

#### مثال:

الطلب:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getBlockCommitment","params":[5]}
'
```

النتيجة:

```json
{ "jsonrpc": "2.0", "result": 1574721591, "id": 1 }
```

### الحصول على عُقَد المجموعة (getClusterNodes)

يُرجع معلومات حول جميع العُقد (nodes) المُشاركة في المجموعة (cluster)

#### المُعلمات (parameters):

لا شيء

#### النتائج:

سيكون حقل النتيجة عبارة عن مجموعة من كائنات JSON، لكل منها الحقول الفرعية التالية:

- `pubkey: <string>` - المفتاح العمومي (public key) للعُقدة (node)، كسلسلة مُرمّزة base-58
- القيل والقال `gossip: <string>` - عنوان شبكة القيل والقال (Gossip network) للعُقدة (node)
- `tpu: <string>` - عنوان شبكة الtpu للعُقدة (node)
- `rpc: <string>|null` - عنوان شبكة JSON RPC للعُقدة (node)، أو لاغ`null` إذا لم يتم تمكين خدمة الـ JSON RPC
- الإصدار `version: <string>|null` - إصدار البرنامج من العُقدة، أو لاغِ `null` إذا كانت معلومات الإصدار غير مُتوفرة

#### مثال:

الطلب:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getClusterNodes"}
'
```

النتيجة:

```json
{
  "jsonrpc": "2.0",
  "result": [
    {
      "gossip": "10.239.6.48:8001",
      "pubkey": "9QzsJf7LPLj8GkXbYT3LFDKqsj2hHG7TA3xinJHu8epQ",
      "rpc": "10.239.6.48:8899",
      "tpu": "10.239.6.48:8856",
      "version": "1.0.0 c375ce1f"
    }
  ],
  "id": 1
}
```

### الحصول على الكتلة المُؤَكدة (getConfirmedBlock)

يُرجع معلومات الهوية والمُعاملات حول كتلة (block) مُؤكدة في دفتر الأستاذ (ledger)

#### المُعلمات (parameters):

- `<u64>` - الفُتحة (slot)، كرقم صحيح u64
- `<string>` - الترميز لكل مُعاملة تم إرجاعها، إما "jsonsonParsed", "base58" بطيء (_slow_)، "base64". إذا لم يتم توفير المُعلِّمة (parameter)، فإن الترميز المُفترض (Default) هو "json". يُحاول ترميز "jsonParsed" إستخدام مُحلِّلي (parsers) التعليمات الخاصة بالبرامج لإرجاع بيانات أكثر قابلية للقراءة ووضوحا في قائمة تعليمات.معاملة. الرسالة `transaction.message.instructions`. إذا كانت "jsonParsed" مطلوبة ولكن لا يمكن العثور على مُحلل (parser)، فإن التعليمات تعود إلى ترميز JSON العادي للحسابات `accounts` ، البيانات `data` وحقول مُعرف البرنامج `programIdIndex`.

#### النتائج:

سيكون ناتج الإستجابة (response output) كائن Json مع الحقول التالية:

- `<null>` - إذا لم يتم تأكيد الكتلة (block) المُحَدَّدة
- `<object>` - إذا تم تأكيد الكتلة (block)، كائن مع الحقول التالية:
  - تجزئة الكتلة `blockhash: <string>` - تجزئة الكتلة (blockhash) لهذه الكتلة، كسلسلة مُرمّزة base-58
  - تجزئة الكتلة السابقة `previousBlockhash: <string>` - تجزئة الكتلة (blockhash) من أصل هذه الكتلة (block)، كسلسلة مُرمّزة base-58؛ إذا كانت الكتلة (block) الأصلية غير مُتوفرة بسبب تنظيف دفتر الأستاذ (ledger)، فسيعود هذا الحقل إلى "111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111 "
  - الفُتحة الأصل `parentSlot: <u64>` - فهرس الفُتحة (slot) لهذه الكتلة الأصل
  - المُعاملات `transactions: <array>` - مجموعة من الكائنات JSON تحتوي على:
    - المُعاملة `transaction: <object|[string,encoding]>` - [Transaction](#transaction-structure) كائن، إما بصيغة JSON أو البيانات الثنائية المُرمّزة (encoded binary data)، إعتماداً على مُعلِّمة (parameter) الترميز
    - `meta: <object>` - كائن بيانات تعريف حالة المُعاملة، يحتوي على لاغِ `null` أو:
      - خطأ `err: <object | null>` - خطأ إذا فشلت المُعاملة، لاغِ إذا نجحت المُعاملة. [TransactionError definitions](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
      - رسوم `fee: <u64>` - رسوم هذه المُعاملة، كما هو عدد صحيح في u64
      - ما قبل الرصيد `preBalances: <array>` - مجموعة من أرصدة حساب u64 قبل مُعالجة المُعاملة
      - ما بعد الرصيد `postBalances: <array>` - مجموعة من أرصدة حساب u64 بعد معالجة المعاملة
      - التعليمات الداخلية `innerInstructions: <array|undefined>` - قائمة التعليمات [inner instructions](#inner-instructions-structure) الداخلية أو محذوفة إذا لم يتم تمكين تسجيل التعليمات الداخلية أثناء هذه المُعاملة
      - سجلات الرسائل `logMessages: <array>` - مجموعة من رسائل سجل السلسلة أو تم حذفها إذا لم يتم تمكين تسجيل رسائل السجل أثناء هذه المُعاملة
      - إهمال: حالة `status: <object>` - حالة المُعاملة
        - `"Ok": <null>` - العملية كانت ناجحة
        - خطأ `"err": <ERR>` - فشلت المُعاملة مع خطأ (TransactionError) في المُعاملة
  - المُكافآت `rewards: <array>` - مجموعة من كائنات JSON تحتوي على:
    - المفتاح العمومي `pubkey: <string>` - المفتاح العمومي (public key)، كسلسلة مُرمّزة base-58، للحساب الذي تلقى المُكافأة
    - `الlamports: <i64>`عدد الـ lamports المُخَصَّصَة لهذا الحساب، كـ u64
    - ما بعد الرصيد `postBalance: <u64>` - رصيد الحساب في الـ lamports بعد تطبيق المُكافأة
    - نوع المُكافأة `rewardType: <string|undefined>` - نوع المُكافأة: "الرسوم" (fee)، "الإيجار" (rent)، "التصويت" (voting)، "التَّحْصِيص" (staking)
  - وقت الكتلة `blockTime: <i64 | null>` - الوقت التقديري للإنتاج، كتوقيت الختم الزمني Unix (ثواني منذ الفترة Unix). لاغية إذا لم تكن مُتوفرة

#### مثال:

الطلب:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc": "2.0","id":1,"method":"getConfirmedBlock","params":[430, "json"]}
'
```

النتيجة:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "blockTime": null,
    "blockhash": "3Eq21vXNB5s86c62bVuUfTeaMif1N2kUqRPBmGRJhyTA",
    "parentSlot": 429,
    "previousBlockhash": "mfcyqEXB3DnHXki6KjjmZck6YjmZLvpAByy2fj4nh6B",
    "rewards": [],
    "transactions": [
      {
        "meta": {
          "err": null,
          "fee": 5000,
          "innerInstructions": [],
          "logMessages": [],
          "postBalances": [499998932500, 26858640, 1, 1, 1],
          "preBalances": [499998937500, 26858640, 1, 1, 1],
          "status": {
            "Ok": null
          }
        },
        "transaction": {
          "message": {
            "accountKeys": [
              "3UVYmECPPMZSCqWKfENfuoTv51fTDTWicX9xmBD2euKe",
              "AjozzgE83A3x1sHNUR64hfH7zaEBWeMaFuAN9kQgujrc",
              "SysvarS1otHashes111111111111111111111111111",
              "SysvarC1ock11111111111111111111111111111111",
              "Vote111111111111111111111111111111111111111"
            ],
            "header": {
              "numReadonlySignedAccounts": 0,
              "numReadonlyUnsignedAccounts": 3,
              "numRequiredSignatures": 1
            },
            "instructions": [
              {
                "accounts": [1, 2, 3, 0],
                "data": "37u9WtQpcm6ULa3WRQHmj49EPs4if7o9f1jSRVZpm2dvihR9C8jY4NqEwXUbLwx15HBSNcP1",
                "programIdIndex": 4
              }
            ],
            "recentBlockhash": "mfcyqEXB3DnHXki6KjjmZck6YjmZLvpAByy2fj4nh6B"
          },
          "signatures": [
            "2nBhEBYYvfaAe16UMNqRHre4YNSskvuYgx3M6E4JP1oDYvZEJHvoPzyUidNgNX5r9sTyN1J9UxtbCXy2rqYcuyuv"
          ]
        }
      }
    ]
  },
  "id": 1
}
```

#### مثال:

الطلب:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc": "2.0","id":1,"method":"getConfirmedBlock","params":[430, "json"]}
'
```

النتيجة:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "blockTime": null,
    "blockhash": "3Eq21vXNB5s86c62bVuUfTeaMif1N2kUqRPBmGRJhyTA",
    "parentSlot": 429,
    "previousBlockhash": "mfcyqEXB3DnHXki6KjjmZck6YjmZLvpAByy2fj4nh6B",
    "rewards": [],
    "transactions": [
      {
        "meta": {
          "err": null,
          "fee": 5000,
          "innerInstructions": [],
          "logMessages": [],
          "postBalances": [499998932500, 26858640, 1, 1, 1],
          "preBalances": [499998937500, 26858640, 1, 1, 1],
          "status": {
            "Ok": null
          }
        },
        "transaction": [
          "AVj7dxHlQ9IrvdYVIjuiRFs1jLaDMHixgrv+qtHBwz51L4/ImLZhszwiyEJDIp7xeBSpm/TX5B7mYzxa+fPOMw0BAAMFJMJVqLw+hJYheizSoYlLm53KzgT82cDVmazarqQKG2GQsLgiqktA+a+FDR4/7xnDX7rsusMwryYVUdixfz1B1Qan1RcZLwqvxvJl4/t3zHragsUp0L47E24tAFUgAAAABqfVFxjHdMkoVmOYaR1etoteuKObS21cc1VbIQAAAAAHYUgdNXR0u3xNdiTr072z2DVec9EQQ/wNo1OAAAAAAAtxOUhPBp2WSjUNJEgfvy70BbxI00fZyEPvFHNfxrtEAQQEAQIDADUCAAAAAQAAAAAAAACtAQAAAAAAAAdUE18R96XTJCe+YfRfUp6WP+YKCy/72ucOL8AoBFSpAA==",
          "base64"
        ]
      }
    ]
  },
  "id": 1
}
```

#### هيكل المُعاملة (Transaction Structure)

المُعاملات تختلف تماما عن تلك التي تتم في البلوكشاينات الأخرى. تأكد من مُراجعة تحليل المُعاملة [Anatomy of a Transaction](developing/programming-model/transactions.md) لمعرفة المُعاملات في Solana.

يتم تعريف بنية JSON للمُعاملة على النحو التالي:

- التوقيعات `signatures: <array[string]>` - قائمة التوقيعات المُرمّزة base-58 المُطبقة على المُعاملة. القائمة دائما بطول `message.header.numRequiredSignatures` وليست فارغة. التوقيع في الفهرس `i` يتوافق مع المفتاح العمومي (public key) في الفهرس `i` في `message.account_keys`. الأول يُستخدم كمُعرف المُعاملة [transaction id](../../terminology.md#transaction-id).
- الرسالة `message: <object>` - يتُحدد مُحتوى المُعاملة.
  - مفاتيح الحساب `accountKeys: <array[string]>` - قائمة المفاتيح العمومية (public keys) المُرمّزة base-58 المُستخدمة في المُعاملة، بما في ذلك التعليمات والتوقيعات. أول `message.header.numRequiredSignatures` المفاتيح العمومية (public keys) يجب أن تُوقع المُعاملة.
  - الرأس `header: <object>` - تفاصيل أنواع الحساب والتوقيعات المطلوبة من المُعاملة.
    - عدد التوقيعات المطلوبة `numRequiredSignatures: <number>` - مجموع عدد التوقيعات المطلوبة لجعل المُعاملة صحيحة. يجب أن يتطابق عدد التوقيعات المطلوبة مع `numRequiredSignatures` مفاتيح حساب الرسالة `message.account_keys`.
    - `numReadonlySignedAccounes: <number>` - آخر `حسابات numReadonlySignedededaccounts` من المفاتيح المُوقّعة هي حسابات للقراءة-فقط. قد تُعالج البرامج مُعاملات مُتعددة تقوم بتحميل حسابات للقراءة-فقط ضمن مُدخل واحد في PoH، ولكن لا يُسمح لها بتخصيص أو خصم الـ lamports أو تعديل بيانات الحساب. يتم تقييم المُعاملات التي تستهدف نفس حساب القراءة والكتابة بالتسلسل.
    - `numReadonlySignedAccounes: <number>` - آخر `حسابات numReadonlySignedededaccounts` من المفاتيح الغير المُوَقِّعة هي حسابات للقراءة فقط.
  - تجزئة الكتلة الحديثة `RecentBlockhash: <string>` - تجزئة مُرمّزة base-58 لكتلة (block) حديثة في دفتر الأستاذ (ledger) تُستخدم لمنع إزدواج المُعاملات وإعطاء عمر المُعاملات.
  - التعليمات `instructions: <array[object]>` - قائمة تعليمات البرنامج التي سيتم تنفيذها بالتسلسل والمُلتزمة في مُعاملة ذرية واحدة إذا كان كل شيء ناجحا.
    - فهرس مُعرف البرنامج `programIdIndex: <number>` - فهرس في مفاتيح حساب الرسالة `message.accountKeys` مع الإشارة إلى حساب البرنامج الذي يُنفذ هذه التعليمة.
    - الحسابات `accounts: <array[number]>` - قائمة المؤشرات المُرتبة في مفاتيح حساب الرسالة `message.accountKeys` تُبين أي الحسابات التي ستمر إلى البرنامج.
    - البيانات `data: <string>` - بيانات إدخال البرنامج مُرمّزة في سلسلة base-58.

#### هيكل التعليمات الداخلية (Inner Instructions Structure)

يُسجل وقت تشغيل Solana التعليمات المُشتركة بين البرامج التي يتم إستدعاؤها أثناء مُعالجة المُعاملة ويجعلها مُتاحة لمزيد من الشفافية لما تم تنفيذه على الشبكة حسب كل تعليمات المُعاملة. يتم تجميع التعليمات المُستدعاة حسب تعليمات المُعاملة الأصلية ويتم سردها حسب ترتيب المُعالجة.

يُعرَّف هيكل التعليمات الداخلية لـ JSON بأنه قائمة بالعناصر في الهيكل التالي:

- الفهرس: رقم `index: number` - فهرس تعليمة المُعاملات التي نشأت منها التعليمات الداخلية
- التعليمات `instructions: <array[object]>` - قائمة تعليمات البرنامج الداخلي التي تم التذرع بها أثناء تعليمات مُعاملة واحدة.
  - فهرس مُعرف البرنامج `programIdIndex: <number>` - فهرس في مفاتيح حساب الرسالة`message.accountKeys` مع الإشارة إلى حساب البرنامج الذي ينفذ هذه التعليمة.
  - الحِسابات `accounts: <array[number]>` - قائمة المُؤشرات المُرتبة في مفاتيح حِساب الرسالة `message.accountKeys` تُبين أي الحِسابات التي ستمر إلى البرنامج.
  - البيانات `data: <string>` - بيانات إدخال البرنامج مُرمّزة في سلسلة base-58.

### الحصول على الكتل المُؤَكدة (getConfirmedBlocks)

يرجع قائمة بالكتل (blocks) المُؤَكدة بين فُتحتين (slots)

#### المُعلمات (parameters):

- `<u64>` - تشغيل الفُتحة (start_slot)، كعدد صحيح u64
- `<u64>` - (إختياري) تشغيل الفُتحة، كعدد صحيح u64

#### النتائج:

سيكون حقل النتيجة مجموعة من عدد صحيح من u64 يُورد كتل (blocks) مُؤَكدة بين `start_slot` وإما `end_slo`، إذا قدمت، أو أحدث كتلة مُؤَكدة ، شاملة. الحد الأقصى المسموح به هو 500,000 فُتحة (slots).

#### مثال:

الطلب:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc": "2.0","id":1,"method":"getConfirmedBlock","params":[5, "json"]}
'
```

النتيجة:

```json
{ "jsonrpc": "2.0", "result": 1574721591, "id": 1 }
```

### الحصول على الكتل المُؤَكدة بحدود (getConfirmedBlocksWithLimit)

يُرجع قائمة بالكتل (blocks) المُؤَكدة بدءاً من فُتحة (slot) مُعينة

#### المُعلمات (parameters):

- `<u64>` - تشغيل الفُتحة (start_slot)، كعدد صحيح u64
- `<u64>` - حد الفُتحة (slot) كعدد صحيح u64

#### النتائج:

سيكون حقل النتيجة مجموعة من عدد صحيح من u64 يورد كتل (blocks) مؤكدة بدءاً من `start_slo` حتى `limit` الكتل (blocks)، الشامل.

#### مثال:

الطلب:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc": "2.0","id":1,"method":"getConfirmedBlocksWithLimit","params":[5, 3]}
'
```

النتيجة:

```json
{ "jsonrpc": "2.0", "result": [5, 6, 7], "id": 1 }
```

### الحصول على التوقيعات المُؤَكدة للعنوان (getConfirmedSignaturesForAddress)

**إهمال: الرجاء إستخدام getConfirmmedSignaturesForAdds2 بدلا من ذلك**

يُرجع قائمة بجميع التوقيعات المُؤَكدة للمُعاملات التي تتضمن عنوان، ضمن نطاق فُتحة (Slot) مُحَدَّدَة. الحد الأقصى المسموح به هو 10,000 فتحة (slots)

#### المُعلمات (parameters):

- `<string>` - المفتاح العمومي (pubkey) للحساب الذي سيتم الإستفسار عنه، كسلسلة مُرمّزة base-58
- `<u64>` - تشغيل الفُتحة (slot)، كرقم صحيح u64
- `<u64>` - إنهاء الفُتحة (slot)، شاملة

#### النتائج:

سيكون حقل النتائج مجموعة لما يلي:

- `<string>` - توقيع المُعاملة كسلسلة مُرمّزة base-58

سيتم ترتيب التواقيع بناءً على الفُتحة (Slot) التي تم تأكيدها فيها، من الأقل إلى الأعلى

#### مثال:

الطلب:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getConfirmedSignaturesForAddress",
    "params": [
      "6H94zdiaYfRfPfKjYLjyr2VFBg6JHXygy84r3qhc3NsC",
      0,
      100
    ]
  }
'
```

النتيجة:

```json
{
  "jsonrpc": "2.0",
  "result": [
    "35YGay1Lwjwgxe9zaH6APSHbt9gYQUCtBWTNL3aVwVGn9xTFw2fgds7qK5AL29mP63A9j3rh8KpN1TgSR62XCaby",
    "4bJdGN8Tt2kLWZ3Fa1dpwPSEkXWWTSszPSf1rRVsCwNjxbbUdwTeiWtmi8soA26YmwnKD4aAxNp8ci1Gjpdv4gsr",
    "4LQ14a7BYY27578Uj8LPCaVhSdJGLn9DJqnUJHpy95FMqdKf9acAhUhecPQNjNUy6VoNFUbvwYkPociFSf87cWbG"
  ],
  "id": 1
}
```

### الحصول على التوقيعات المُؤَكدة للعنوان 2 (getConfirmedSignaturesForAddress2)

إرجاع التوقيعات المُؤَكدة للمعاملات التي تتضمن ملف العنوان إلى الوراء في الوقت المُناسب من التوقيع المُقدم أو أحدث كتلة (block) مُؤَكدة

#### المُعلمات (parameters):

- `<string>` - المفتاح العمومي (pubkey) للحساب الذي سيتم الإستفسار عنه، كسلسلة مُرمّزة base-58
- `<object>` - كائن تكوين (إختياري) يحتوي على الحقول الإختيارية التالية:
  - الحد الأقصى `limit: <number>` - (إختياري) الحد الأقصى لتوقيعات المُعاملة للإرجاع (بين 1 و 1000، الإفتراض: 1000).
  - قبل `before: <string>` - (إختياري) بدء البحث إلى الوراء من توقيع هذه المُعاملة. في حالة عدم تقديمه، يبدأ البحث من أقصى أعلى كتلة (block) مُؤَكدة.
  - حتى`until: <string>` - (إختياري) البحث حتى توقيع هذه المُعاملة، إذا وجدت قبل الوصول إلى الحد الأقصى.

#### النتائج:

سيكون حقل النتيجة عبارة عن مجموعة من معلومات توقيع المُعاملة، مُرتبة من المُعاملة الأحدث إلى الأقدم:

- `<object>`
  - `signature: <string>` - توقيع المُعاملة كسلسلة مُرمّزة base-58
  - الفُتحة `slot: <u64>` - الفُتحة (slot) التي تحتوي على الكتلة (block) مع المُعاملة
  - خطأ `err: <object | null>` - خطأ إذا فشلت المُعاملة، لاغِ إذا نجحت المُعاملة. تعريفات خطأ المُعاملة [TransactionError definitions](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
  - المُذكرة `memo: <string |null>` - مُذكرة مُرتبطة بالمُعاملة، لاغية إذا لم تكن هناك مُذكرة موجودة

#### مثال:

الطلب:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getConfirmedSignaturesForAddress",
    "params": [
      "6H94zdiaYfRfPfKjYLjyr2VFBg6JHXygy84r3qhc3NsC",
      0,
      100
    ]
  }
'
```

النتيجة:

```json
{
  "jsonrpc": "2.0",
  "result": [
    {
      "err": null,
      "memo": null,
      "signature": "5h6xBEauJ3PK6SWCZ1PGjBvj8vDdWG3KpwATGy1ARAXFSDwt8GFXM7W5Ncn16wmqokgpiKRLuS83KUxyZyv2sUYv",
      "slot": 114
    }
  ],
  "id": 1
}
```

### الحصول على المُعاملة المُؤَكدة (getConfirmedTransaction)

إرجاع تفاصيل المعاملة للمعاملة المؤكدة

#### المُعلمات (parameters):

- يحاول ترميز base-58 `<string>` إستخدام معلِّمات (parameters) التعليمات الخاصة بالبرامج لإرجاع بيانات أكثر قابلية للقراءة ووضوحا في قائمة المُعاملة. الرسالة. التعليمات `transaction.message.instructions`. إذا كانت "jsonParsed" مطلوبة ولكن لا يُمكن العثور على مُحلل (parser)، فإن التعليمات تعود إلى ترميز JSON العادي (حقول الحسابات `accounts`، البيانات `data`، فهرس مُعرف البرنامج `programIdIndex`).
- `<string>` - الترميز لكل مُعاملة تم إرجاعها، إما "json"، "jsonParsed"، "base58" (_slow_)، أو "base64". إذا لم يتم توفير المُعلِّمة (parameter)، فإن الترميز المُفترض (Default) هو "json.

#### النتائج:

- `<null>` - إذا لم يتم العثور على المُعاملة أو لم يتم تأكيدها
- `<object>` - إذا تم تأكيد المُعاملة، كائن مع الحقول التالية:
  - `slot: <u64>` - فُتحة (slot) هذه المُعاملة تمت مُعالجتها في
  - `transaction: <object|[string,encoding]>` - [transaction](#transaction-structure) الكائن، إما بصيغة JSON أو البيانات الثنائية المُرمّزة، إعتماداً على مُعلِّمة (parameter) الترميز
  - `meta: <object | null>` - كائن البيانات الوصفية لحالة المُعاملة:
    - خطأ `err: <object | null>` - خطأ إذا فشلت المُعاملة، لاغ إذا نجحت المُعاملة. تعريفات خطأ المُعاملة [TransactionError definitions](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
    - الرسوم `fee: <u64>` - رسوم هذه المُعاملة، كما هو عدد صحيح في u64
    - ما قبل الأرصِدة `preBalances: <array>` - مجموعة من أرصدة حساب u64 قبل معالجة المعاملة
    - `postBalances: <array>` - مجموعة من أرصدة حساب u64 بعد مُعالجة المُعاملة
    - التعليمات الداخلية `innerInstructions: <array|undefined>` - قائمة التعليمات الداخلية [inner instructions](#inner-instructions-structure) أو محذوفة إذا لم يتم تمكين تسجيل التعليمات الداخلية أثناء هذه المُعاملة
    - `logMessages: <array>` - مجموعة من رسائل سجل السلسلة أو تم حذفها إذا لم يتم تمكين مُسجل رسائل السجل أثناء هذه المُعاملة
    - إهمال: الحالة `status: <object>` - حالة المُعاملة
      - `"Ok": <null>` - المُعاملة كانت ناجحة
      - خطأ `"err": <ERR>` - فشلت المُعاملة مع خطأ في المُعاملة (TransactionError)

#### مثال:

الطلب:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getConfirmedTransaction",
    "params": [
      "2nBhEBYYvfaAe16UMNqRHre4YNSskvuYgx3M6E4JP1oDYvZEJHvoPzyUidNgNX5r9sTyN1J9UxtbCXy2rqYcuyuv",
      "json"
    ]
  }
'
```

النتيجة:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "meta": {
      "err": null,
      "fee": 5000,
      "innerInstructions": [],
      "postBalances": [499998932500, 26858640, 1, 1, 1],
      "preBalances": [499998937500, 26858640, 1, 1, 1],
      "status": {
        "Ok": null
      }
    },
    "slot": 430,
    "transaction": {
      "message": {
        "accountKeys": [
          "3UVYmECPPMZSCqWKfENfuoTv51fTDTWicX9xmBD2euKe",
          "AjozzgE83A3x1sHNUR64hfH7zaEBWeMaFuAN9kQgujrc",
          "SysvarS1otHashes111111111111111111111111111",
          "SysvarC1ock11111111111111111111111111111111",
          "Vote111111111111111111111111111111111111111"
        ],
        "header": {
          "numReadonlySignedAccounts": 0,
          "numReadonlyUnsignedAccounts": 3,
          "numRequiredSignatures": 1
        },
        "instructions": [
          {
            "accounts": [1, 2, 3, 0],
            "data": "37u9WtQpcm6ULa3WRQHmj49EPs4if7o9f1jSRVZpm2dvihR9C8jY4NqEwXUbLwx15HBSNcP1",
            "programIdIndex": 4
          }
        ],
        "recentBlockhash": "mfcyqEXB3DnHXki6KjjmZck6YjmZLvpAByy2fj4nh6B"
      },
      "signatures": [
        "2nBhEBYYvfaAe16UMNqRHre4YNSskvuYgx3M6E4JP1oDYvZEJHvoPzyUidNgNX5r9sTyN1J9UxtbCXy2rqYcuyuv"
      ]
    }
  },
  "id": 1
}
```

#### مثال:

الطلب:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getConfirmedTransaction",
    "params": [
      "2nBhEBYYvfaAe16UMNqRHre4YNSskvuYgx3M6E4JP1oDYvZEJHvoPzyUidNgNX5r9sTyN1J9UxtbCXy2rqYcuyuv",
      "base64"
    ]
  }
'
```

النتيجة:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "meta": {
      "err": null,
      "fee": 5000,
      "innerInstructions": [],
      "postBalances": [499998932500, 26858640, 1, 1, 1],
      "preBalances": [499998937500, 26858640, 1, 1, 1],
      "status": {
        "Ok": null
      }
    },
    "slot": 430,
    "transaction": [
      "AVj7dxHlQ9IrvdYVIjuiRFs1jLaDMHixgrv+qtHBwz51L4/ImLZhszwiyEJDIp7xeBSpm/TX5B7mYzxa+fPOMw0BAAMFJMJVqLw+hJYheizSoYlLm53KzgT82cDVmazarqQKG2GQsLgiqktA+a+FDR4/7xnDX7rsusMwryYVUdixfz1B1Qan1RcZLwqvxvJl4/t3zHragsUp0L47E24tAFUgAAAABqfVFxjHdMkoVmOYaR1etoteuKObS21cc1VbIQAAAAAHYUgdNXR0u3xNdiTr072z2DVec9EQQ/wNo1OAAAAAAAtxOUhPBp2WSjUNJEgfvy70BbxI00fZyEPvFHNfxrtEAQQEAQIDADUCAAAAAQAAAAAAAACtAQAAAAAAAAdUE18R96XTJCe+YfRfUp6WP+YKCy/72ucOL8AoBFSpAA==",
      "base64"
    ]
  },
  "id": 1
}
```

### الحصول على معلومات الفترة (getEpochInfo)

إرجاع معلومات حول الفترة (epoc) الحالية

#### المُعلمات (parameters):

- `<object>` - (إختياري) الإلتزام [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### النتائج:

سيكون ناتج الإستجابة (response output) كائن Json مع الحقول التالية:

- `absoluteSlot: <u64>`، الفُتحة (slot) الحالية
- `blockHeight: <u64>`، ارتفاع الكتلة الحالي
- `epoch: <u64>`، الفترة (epoch) الحالية
- `slotIndex: <u64>`، الفُتحة (slot) الحالية بالنسبة إلى بداية الحقبة (epoch) الحالية
- `slotsInEpoch: <u64>`، عدد الفُتحات (slots) في هذه الحقبة (epoch)

#### مثال:

الطلب:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getEpochInfo"}
'
```

النتيجة:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "absoluteSlot": 166598,
    "blockHeight": 166500,
    "epoch": 27,
    "slotIndex": 2790,
    "slotsInEpoch": 8192
  },
  "id": 1
}
```

### الحصول على جدول الفترة (getEpochSchedule)

إرجاع معلومات جدول الفترة (epoch) من مرحلة التكوين (Genesis) لهذه المجموعة (cluster)

#### المُعلمات (parameters):

لا شيء

#### النتائج:

سيكون ناتج الإستجابة (response output) كائن Json مع الحقول التالية:

- `slotsInEpoch: <u64>`، عدد الفُتحات (slots) في هذه الفترة (epoch)
- `leaderScheduleSlotOffset: <u64>`، عدد الفُتحات (slots) قبل بداية فترة (epoch) ما لحساب جدول القائد (leader schedule) لتلك الفترة (epoch)
- فترة الإحماء `warmup: <bool>`، سواء بدأت الفترات (epochs) قصيرة وتزداد
- `firstNormalEpoch: <u64>`, first normal-length epoch, log2(slotsPerEpoch) - log2(MINIMUM_SLOTS_PER_EPOCH)
- `firstNormalSlot: <u64>`, MINIMUM_SLOTS_PER_EPOCH \* (2.pow(firstNormalEpoch) - 1)

#### مثال:

الطلب:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getEpochSchedule"}
```

النتيجة:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "firstNormalEpoch": 8,
    "firstNormalSlot": 8160,
    "leaderScheduleSlotOffset": 8192,
    "slotsPerEpoch": 8192,
    "warmup": true
  },
  "id": 1
}
```

### الحصول على حاسبة الرسوم لتجزئة الكتلة (getFeeCalculatorForBlockhash)

يُرجع آلة حاسبة الرسوم المُرتبطة بتجزئة الكتلة (blockhash)، أو لاغِ `null` إذا إنتهت صلاحية تجزئة الكتلة (blockhash)

#### المُعلمات (parameters):

- `<string>` - الإستعلام عن تجزئة الكتلة (blockhash) كسلسلة مُرمّزة Base58
- `<object>` - (إختياري) الإلتزام [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### النتائج:

ستكون النتيجة كائن RpcResponse JSON بقيمة `value` تساوي:

- `<null>` - إذا إنتهى الإستعلام عن تجزئة الكتلة (blockhash)
- `<object>` - خلاف ذلك، كائن JSON يحتوي على:
  - حاسبة الرسوم `feeCalculator: <object>`، `FeeCalculator` الكائن الذي يصف مُعدل رسوم المجموعة (cluster) في تجزئة الكتلة (blockhash) المُستفسر عنه

#### مثال:

الطلب:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getFeeCalculatorForBlockhash",
    "params": [
      "GJxqhuxcgfn5Tcj6y3f8X4FeCDd2RQ6SnEMo1AAxrPRZ"
    ]
  }
'
```

النتيجة:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 221
    },
    "value": {
      "feeCalculator": {
        "lamportsPerSignature": 5000
      }
    }
  },
  "id": 1
}
```

### الحصول على مُحافظ معدل الرسوم (getFeeRateGovernor)

يُرجع معلومات مُحافظ مُعدل الرسوم من البنك الجذر (root bank)

#### المُعلمات (parameters):

لا شيء

#### النتائج:

ستكون النتيجة `result` كائن `object` مع الحقول التالية:

- نسبة الحرق `burnPercent: <u8>` النسبة المئوية للرسوم التي تم جمعها لتدميرها
- `maxLamportsPerSignature: <u64>`، القيمة الأكبر `lamportsPerSignature` يُمكن أن تحصل على الفُتحة (slot) التالية
- `minLamportsPerSignature: <u64>`، القيمة الأصغر`lamportsPerSignature` يُمكن أن تحصل على الفُتحة (slot) التالية
- `targetLamportsPerSignature: <u64>`، مُعدل الرسوم المطلوب للمجموعة (cluster)
- `targetSignaturesPeruct: <u64>`، معدل التوقيع المطلوب للمجموعة (cluster)

#### مثال:

الطلب:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getFeeRateGovernor"}
'
```

النتيجة:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 54
    },
    "value": {
      "feeRateGovernor": {
        "burnPercent": 50,
        "maxLamportsPerSignature": 100000,
        "minLamportsPerSignature": 5000,
        "targetLamportsPerSignature": 10000,
        "targetSignaturesPerSlot": 20000
      }
    }
  },
  "id": 1
}
```

### الحصول على الرسوم (getFees)

يُرجع تجزئة كتلة (blockhash) حديثة من دفتر الأستاذ (ledger)، وجدول رسوم يُمكن إستخدامه لحساب تكلفة إرسال مُعاملة، وآخر فُتحة (slot) تكون فيها تجزئة كتلة (blockhash) صالحة.

#### المُعلمات (parameters):

- `<object>` - (إختياري) الإلتزام [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### النتائج:

ستكون النتيجة كائن JSON RpcResponse مع قيمة `value` مجموعة إلى كائن JSON مع الحقول التالية:

- تجزئة الكُتلة `blockhash: <string>` - تجزئة الكُتلة (blockhash) كسلسلة مُرمّزة base-58
- `feeCalculator: <object>` - عنصر حاسبة الرسوم (FeeCalculator)، جدول رسوم لتجزئة الكُتلة (blockhash) هذه
- آخر فُتحة صالحة `lastValiduct: <u64>` - الفُتحة (slot) الأخيرة التي ستكون فيها تجزئة الكُتلة (blockhash) صالحة

#### مثال:

الطلب:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getFees"}
'
```

النتيجة:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1
    },
    "value": {
      "blockhash": "CSymwgTNX1j3E4qhKfJAUE41nBWEwXufoYryPbkde5RR",
      "feeCalculator": {
        "lamportsPerSignature": 5000
      },
      "lastValidSlot": 297
    }
  },
  "id": 1
}
```

### الحصول على أول كتلة مُتاحة (getFirstAvailableBlock)

يُرجع فُتحة (slot) أقل كتلة (block) مُؤكدة لم يتم إزالتها من دفتر الأستاذ (ledger)

#### المُعلمات (parameters):

لا شيء

#### النتائج:

- `<u64>` - الفُتحة (Slot)

#### مثال:

الطلب:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getFirstAvailableBlock"}
'
```

النتيجة:

```json
{ "jsonrpc": "2.0", "result": 250000, "id": 1 }
```

### الحصول على تجزئة مرحلة التكوين (getGenesisHash)

يُرجع تجزئة مرحلة التكوين (genesis hash)

#### المُعلمات (parameters):

لا شيء

#### النتائج:

- `blockhash: <string>` - تجزئة الكتلة (blockhash) كسلسلة مُرمّزة base-58

#### مثال:

الطلب:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getGenesisHash"}
'
```

النتيجة:

```json
{
  "jsonrpc": "2.0",
  "result": "GH7ome3EiwEr7tu9JuTh2dpYWBJK3z69Xm1ZE3MEE6JC",
  "id": 1
}
```

### الحصول على الصِّحَّة (getHealth)

يُرجع صِحَّة العُقدة (node) الحالية.

إذا تم توفير حُجَج المُدقّقين الموثوق بهم `--trusted-validator` إلى مُدقّق `solana-validator`، فسيتم إرجاع "ok" (مُوافق) عندما تكون العُقدة (node) ضمن مسافة فُتحات الفحص الصِّحَّي `HEALTH_CHECK_SLOT_DISTANCE` لأعلى مُدقّق موثوق به، وإلا فسيتم إرجاع خطأ. رسالة "ok" (حسنا) تُعاد دائماً إذا لم يتم توفير أي مُدقّقين (validators) موثوقين.

#### المُعلمات (parameters):

لا شيء

#### النتائج:

إذا كانت العُقدة (node) سليمة صِحَّيا: "ok" (حسنًا) إذا كانت العُقدة (node) غير سليمة صِحَّيا، يتم إرجاع رد خطأ في JSON RPC. تفاصيل الرد على الخطأ (error) هي غير مُستقر **UNSTABLE** وقد تتغير في المُستقبل

#### مثال:

الطلب:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getFees"}
'
```

النتيجة الصِحَّية:

```json
{ "jsonrpc": "2.0", "result": 250000, "id": 1 }
```

النتيجة غير الصِحَّية (الصِحَّة العامة):

```json
{
  "jsonrpc": "2.0",
  "error": {
    "code": -32005,
    "message": "Node is unhealthy",
    "data": {}
  },
  "id": 1
}
```

نتيجة غير صِحَّية (إذا توافرت معلومات إضافية)

```json
{
  "jsonrpc": "2.0",
  "error": {
    "code": -32005,
    "message": "Node is behind by 42 slots",
    "data": {
      "numSlotsBehind": 42
    }
  },
  "id": 1
}
```

### الحصول على الهوية (getIdentity)

إرجاع مفتاح الهوية العمومي (pubkey) للعُقدة (node) الحالية

#### المُعلمات (parameters):

لا شيء

#### النتائج:

سيكون ناتج الإستجابة (response output) كائن Json مع الحقول التالية:

- الهوية `identity`، المفتاح العمومي (Pubkeys) لهوية (node) الحالية \(كسلسلة مُرمّزة base-58\)

#### مثال:

الطلب:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getIdentity"}
'
```

النتيجة:

```json
{
  "jsonrpc": "2.0",
  "result": { "identity": "2r1F4iWqVcb8M1DbAjQuFpebkQHY9hcVU4WW2DJBpN" },
  "id": 1
}
```

### الحصول على مُحافظ التَضَخُّم (getInflationGovernor)

يُرجع حاكم التَضَخُّم الحالي

#### المُعلمات (parameters):

- `<object>` - (إختياري) الإلتزام [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### النتائج:

سيكون ناتج الإستجابة (response output) كائن Json مع الحقول التالية:

- `initial: <f64>`، نسبة التَضَخُّم الأولية من الوقت 0
- `terminal: <f64>`، نسبة التَضَخُّم النهائي
- `taper: <f64>`، المُعدل السنوي الذي يتم فيه خفض التَضَخُّم
- `foundation: <f64>`، النسبة المئوية من إجمالي التَضَخُّم المُخَصَّصَة للمؤسسة
- `foundationTerm: <f64>`، مُدة تَضَخُّم مُجَمَّع المُؤسسة في سنوات

#### مثال:

الطلب:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getInflationGovernor"}
'
```

النتيجة:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "foundation": 0.05,
    "foundationTerm": 7,
    "initial": 0.15,
    "taper": 0.15,
    "terminal": 0.015
  },
  "id": 1
}
```

### الحصول على مُعدل التَضَخُّم (getInflationRate)

يُرجع قيم التَضَخُّم المُحَدَّدة للفترة (epoch) الحالية

#### المُعلمات (parameters):

لا شيء

#### النتائج:

سيكون ناتج الإستجابة (response output) كائن Json مع الحقول التالية:

- `total: <f64>`، إجمالي التَضَخُّم
- `validator: <f64>`، تَضَخُّم مُخَصَّصَ للمُدقّقين (validators)
- `foundation: <f64>`، النسبة المئوية من إجمالي التَضَخُّم المُخَصَّصَة للمُؤسسة
- `epoch: <f64>`، الفترة (epoch) التي تكون فيها هذه القِيَم صالحة

#### مثال:

الطلب:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getInflationRate"}
'
```

النتيجة:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "epoch": 100,
    "foundation": 0.001,
    "total": 0.149,
    "validator": 0.148
  },
  "id": 1
}
```

### الحصول على أكبر الحِسابات (getLargestAccounts)

يُرجع الحِسابات الـ 20 الأكبر، حسب ترتيب رصيد الـ lamport

#### المُعلمات (parameters):

- `<object>` - (إختياري) تحتوي إعدادات الكائن على الحقول الإختيارية التالية:
  - (اختياري) الإلتزام [Commitment](jsonrpc-api.md#configuring-state-commitment)
  - (إختياري) `filter: <string>` - نتائج الفلترة حسب نوع الحساب؛ تدعم حاليا المعروض المُتاح في السوق | المعروض الغير مُتاح في السوق `circulating|nonCirculating`

#### النتائج:

ستكون النتيجة كائن RpcResponse JSON بقِيمة `value` تُساوي:

- `<object>` - خلاف ذلك، كائن JSON يحتوي على:
  - `address: <string>`، عنوان الحساب المُرمّز base-58
  - الـ `lamports: <u64>`، عدد الlamports المُخَصَّصَة لهذا الحساب، كـ u64

#### مثال:

الطلب:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getLargestAccounts"}
'
```

النتيجة:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 54
    },
    "value": [
      {
        "lamports": 999974,
        "address": "99P8ZgtJYe1buSK8JXkvpLh8xPsCFuLYhz9hQFNw93WJ"
      },
      {
        "lamports": 42,
        "address": "uPwWLo16MVehpyWqsLkK3Ka8nLowWvAHbBChqv2FZeL"
      },
      {
        "lamports": 42,
        "address": "aYJCgU7REfu3XF8b3QhkqgqQvLizx8zxuLBHA25PzDS"
      },
      {
        "lamports": 42,
        "address": "CTvHVtQ4gd4gUcw3bdVgZJJqApXE9nCbbbP4VTS5wE1D"
      },
      {
        "lamports": 20,
        "address": "4fq3xJ6kfrh9RkJQsmVd5gNMvJbuSHfErywvEjNQDPxu"
      },
      {
        "lamports": 4,
        "address": "AXJADheGVp9cruP8WYu46oNkRbeASngN5fPCMVGQqNHa"
      },
      {
        "lamports": 2,
        "address": "8NT8yS6LiwNprgW4yM1jPPow7CwRUotddBVkrkWgYp24"
      },
      {
        "lamports": 1,
        "address": "SysvarEpochSchedu1e111111111111111111111111"
      },
      {
        "lamports": 1,
        "address": "11111111111111111111111111111111"
      },
      {
        "lamports": 1,
        "address": "Stake11111111111111111111111111111111111111"
      },
      {
        "lamports": 1,
        "address": "SysvarC1ock11111111111111111111111111111111"
      },
      {
        "lamports": 1,
        "address": "StakeConfig11111111111111111111111111111111"
      },
      {
        "lamports": 1,
        "address": "SysvarRent111111111111111111111111111111111"
      },
      {
        "lamports": 1,
        "address": "Config1111111111111111111111111111111111111"
      },
      {
        "lamports": 1,
        "address": "SysvarStakeHistory1111111111111111111111111"
      },
      {
        "lamports": 1,
        "address": "SysvarRecentB1ockHashes11111111111111111111"
      },
      {
        "lamports": 1,
        "address": "SysvarFees111111111111111111111111111111111"
      },
      {
        "lamports": 1,
        "address": "Vote111111111111111111111111111111111111111"
      }
    ]
  },
  "id": 1
}
```

### الحصول على جدول القائد (getLeaderSchedule)

يُرجع الجدول الزمني للقائد في فترة (epoch) ما

#### المُعلمات (parameters):

- `<u64>` - (إختياري) إحضار جدول القائد (leader schedule) للفترة (epoch) التي تتوافق مع الفُتحة (Slot) المُتاحة. إذا لم يتم تحديدها، يتم جلب جدول القائد (leader schedule) للفترة (epoch) الحالية
- `<object>` - (اختياري) الالتزام [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### النتائج:

- `<null>` - إذا لم يتم إيجاد الفترة (epoch) المطلوبة
- `<object>` - خلاف ذلك، سيكون حقل النتيجة قاموس للمفاتيح العمومية (public keys) للقائد (leader) \(كسلاسل مُرمّزة base-58\) ومُؤشرات فُتحة (slot) القائد (leader) المُقابلة لها كقِيَم (المُؤشرات ذات صلة بالفُتحة الأولى في الفترة المطلوبة)

#### مثال:

الطلب:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getEpochSchedule"}
```

النتيجة:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "4Qkev8aNZcqFNSRhQzwyLMFSsi94jHqE8WNVTJzTP99F": [
      0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20,
      21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38,
      39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56,
      57, 58, 59, 60, 61, 62, 63
    ]
  },
  "id": 1
}
```

### الحصول على الحد الأدنى للرصيد للإعفاء من الإيجار (getMinimumBalanceForRentExemption)

يُرجع الحد الأدنى من الرصيد المطلوب لإعفاء الحساب من الإيجار.

#### المُعلمات (parameters):

- `<usize>` - طول بيانات الحساب
- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### النتائج:

- `<u64>` - الحد الأدنى للـ lamports المطلوبة في الحساب

#### مثال:

الطلب:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getBlockCommitment","params":[50]}
'
```

النتيجة:

```json
{ "jsonrpc": "2.0", "result": 500, "id": 1 }
```

### الحصول على حسابات متعددة (getMultipleAccounts)

يُرجع معلومات الحِساب لقائمة من المفاتيح العمومية (Pubkeys)

#### المُعلمات (parameters):

- `<array>` - المفتاح العمومي (pubkey) للحِساب الذي سيتم الإستفسار عنه، كسلسلة مُرمّزة base-58
- `<object>` - (إختياري) تحتوي إعدادات الكائن على الحقول الإختيارية التالية:
  - (إختياري) الإلتزام [Commitment](jsonrpc-api.md#configuring-state-commitment)
  - الترميز `encoding: <string>` - الترميز لبيانات الحساب، إما "base58" بطيء "(_slow_) ، "base64"، "base64+zstd" ، أو "jsonParsed". يقتصر "base58" على بيانات الحساب التي تقل عن 128 bytes. سيقوم "base64" بإرجاع البيانات المُرمّزة لـ base64 لبيانات الحساب من أي حجم. "base64+zstd" يضغط على بيانات الحساب بإستخدام [Zstandard](https://facebook.github.io/zstd/) و base64-en النتيجة. يُحاول ترميز "jsonParsed" إستخدام مُوزعي الحالة الخاصين بالبرنامج لإرجاع المزيد من بيانات حالة الحساب التي يُمكن قراءتها بشكل واضح. إذا طُلب "jsonParsed" ولكن لا يُمكن العثور على مُحلِّل (parser)، يعود الحقل إلى الترميز "base64"، يُمكن الكشف عنها عندما تكون بيانات `data` الحقل من النوع `<string>`.
  - (إختياري) `dataSlice: <object>` - الحد من بيانات الحساب التي تم إرجاعها بإستخدام المُوازنة المُقدمة `offset: <usize>` و `length: <usize>` الحقول؛ مُتاح فقط لترميز "base58", "base64" أو "base64+zstd".

#### النتائج:

ستكون النتيجة كائن RpcResponse JSON ب `value` تُساوي:

مجموعة من:

- `<null>` - إذا كان الحساب المطلوب لذلك المفتاح العمومي (Pubkey) غير موجود
- `<object>` - خلاف ذلك، كائن JSON يحتوي على:
  - `lamports: <u64>` عدد الـ lamports المخصصة لهذا الحساب، كـ u64
  - المالك `owner: <string>`، المفتاح العمومي (pubkey) المُرمّز base-58 من البرنامج الذي تم تعيين هذا الحساب ل
  - `data: <[string, encoding]|object>`، البيانات المُرتبطة بالحساب، إما كبيانات ثنائية مُرمّزة (encoded) أو بصيغة `{<program>: <state>}`، إعتمادا على مُعلِّمة (parameter) الترميز
  - `executable: <bool>`، المنطقية (boolean) تُشير إلى ما إذا كان الحساب يحتوي على برنامج \(وهو للقراءة-فقط\)
  - فترة الإيجار `rentEpoch: <u64>`، الفترة (epoch) التي سيكون فيها هذا الحساب مدينا بالإيجار القادم، مثل u64

#### مثال:

الطلب:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getAccountInfo",
    "params": [
      "4fYNw3dojWmQ4dXtSGE9epjRGy9pFSx62YypT7avPYvA",
      {
        "encoding": "jsonParsed"
      }
    ]
  }
'
```

النتيجة:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1
    },
    "value": [
      {
        "data": ["AAAAAAEAAAACtzNsyJrW0g==", "base64"],
        "executable": false,
        "lamports": 1000000000,
        "owner": "11111111111111111111111111111111",
        "rentEpoch": 2
      },
      {
        "data": ["", "base64"],
        "executable": false,
        "lamports": 5000000000,
        "owner": "11111111111111111111111111111111",
        "rentEpoch": 2
      }
    ]
  },
  "id": 1
}
```

#### مثال:

الطلب:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getAccountInfo",
    "params": [
      "4fYNw3dojWmQ4dXtSGE9epjRGy9pFSx62YypT7avPYvA",
      {
        "encoding": "jsonParsed"
      }
    ]
  }
'
```

النتيجة:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1
    },
    "value": [
      {
        "data": [
          "11116bv5nS2h3y12kD1yUKeMZvGcKLSjQgX6BeV7u1FrjeJcKfsHRTPuR3oZ1EioKtYGiYxpxMG5vpbZLsbcBYBEmZZcMKaSoGx9JZeAuWf",
          "base58"
        ],
        "executable": false,
        "lamports": 1000000000,
        "owner": "11111111111111111111111111111111",
        "rentEpoch": 2
      },
      {
        "data": ["", "base58"],
        "executable": false,
        "lamports": 5000000000,
        "owner": "11111111111111111111111111111111",
        "rentEpoch": 2
      }
    ]
  },
  "id": 1
}
```

### الحصول على حِسابات البرنامج (getProgramAccounts)

يُرجع جميع الحسابات المملوكة للبرنامج المزود للمفتاح العمومي (pubkey)

#### المُعلمات (parameters):

- `<string>` - المفتاح العمومي (pubkey) للبرنامج الذي سيتم الإستفسار عنه، كسلسلة مُرمّزة base-58
- `<object>` - (إختياري) تحتوي إعدادات الكائن على الحقول الإختيارية التالية:
  - (إختياري) الإلتزام [Commitment](jsonrpc-api.md#configuring-state-commitment)
  - الترميز `encoding: <string>` - الترميز لبيانات الحساب، إما "base58" بطيء "(_slow_) ، "base64", "base64+zstd" ، أو "jsonParsed". يقتصر "base58" على بيانات الحساب التي تقل عن 128 bytes. سيقوم "base64" بإرجاع البيانات المُرمّزة لـ base64 لبيانات الحساب من أي حجم. "base64+zstd" يضغط على بيانات الحساب بإستخدام [Zstandard](https://facebook.github.io/zstd/) و base64-en النتيجة. يُحاول ترميز "jsonParsed" إستخدام مُوزعي الحالة الخاصين بالبرنامج لإرجاع المزيد من بيانات حالة الحساب التي يُمكن قراءتها بشكل واضح. إذا طُلب "jsonParsed" ولكن لا يُمكن العثور على مُحلِّل (parser)، يعود الحقل إلى الترميز "base64"، يُمكن الكشف عنها عندما يكون `data` الحقل من نوع `<string>`.
  - (إختياري) `dataSlice: <object>` - الحد من بيانات الحساب التي تم إرجاعها بإستخدام المقدمة `offset: <usize>` و `length: <usize>` الحقول؛ متاح فقط لترميز "base58", "base64" أو "base64+zstd".
  - (اختياري) `filters: <array>` - نتائج الفلترة بإستخدام كائنات فلترة مُختلفة [filter objects](jsonrpc-api.md#filters); الحساب يجب أن يفي بجميع معايير الفلترة لتضمينها في النتائج

##### الفلاتر (Filters):

- `memcmp: <object>` - مُقارنة سلسلة من الـ bytes التي تم توفيرها مع بيانات حِساب البرنامج في إزاحة (offset) مُعينة. الحقول (Fields):

  - `offset: <usize>` - الإزاحة في بيانات حساب البرنامج لبدء المُقارنة
  - `بايت: <string>` - لبدء مقارنة البيانات، كسلسلة مرمّزة base-58

- حجم البيانات `dataSize: <u64>` - مقارنة طول بيانات حساب البرنامج مع حجم البيانات المُقدمة

#### النتائج:

سيكون حقل النتيجة عبارة عن مجموعة من الكائنات JSON، التي ستحتوي على:

- `pubkey: <string>` - المفتاح العمومي (public key) كسلسلة مُرمّزة base-58
- `account: <object>` - كائن JSON، مع الحقول الفرعية التالية:
  - `lamports: <u64>`، عدد الـ lamports المُخَصَّصَة لهذا الحساب، كـ u64
  - المالك `owner: <string>`، المفتاح العمومي المُرمّز base-58 للبرنامج الذي تم تعيين هذا الحساب له `data: <[string,encoding]|object>`، البيانات المُرتبطة بالحِساب، إما كبيانات ثنائية مُشَفَّرَة أو شكل `{<program>: <state>}`، إعتمادًا على مُعلِّمة (parameter) الترميز
  - قابل للتنفيذ `executable: <bool>`، المنطقية (boolean) تُشير إلى ما إذا كان الحِساب يحتوي على برنامج \(وهو فقط-للقراءة\)
  - إيجار الفترة `rentEpoch: <u64>`، الفترة (epoch) التي سيكون فيها هذا الحساب مدينا بالإيجار القادم، ك u64

#### مثال:

الطلب:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getBalance", "params":["83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri"]}
'
```

النتيجة:

```json
{
  "jsonrpc": "2.0",
  "result": [
    {
      "account": {
        "data": "2R9jLfiAQ9bgdcw6h8s44439",
        "executable": false,
        "lamports": 15298080,
        "owner": "4Nd1mBQtrMJVYVfKf2PJy9NZUZdTAsp7D4xWLs4gDB4T",
        "rentEpoch": 28
      },
      "pubkey": "CxELquR1gPP8wHe33gZ4QxqGB3sZ9RSwsJ2KshVewkFY"
    }
  ],
  "id": 1
}
```

#### مثال:

الطلب:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getProgramAccounts",
    "params": [
      "4Nd1mBQtrMJVYVfKf2PJy9NZUZdTAsp7D4xWLs4gDB4T",
      {
        "filters": [
          {
            "dataSize": 17
          },
          {
            "memcmp": {
              "offset": 4,
              "bytes": "3Mc6vR"
            }
          }
        ]
      }
    ]
  }
'
```

النتيجة:

```json
{
  "jsonrpc": "2.0",
  "result": [
    {
      "account": {
        "data": "2R9jLfiAQ9bgdcw6h8s44439",
        "executable": false,
        "lamports": 15298080,
        "owner": "4Nd1mBQtrMJVYVfKf2PJy9NZUZdTAsp7D4xWLs4gDB4T",
        "rentEpoch": 28
      },
      "pubkey": "CxELquR1gPP8wHe33gZ4QxqGB3sZ9RSwsJ2KshVewkFY"
    }
  ],
  "id": 1
}
```

### الحصول على تجزئة الكتلة الحديثة (getRecentBlockhash)

يُرجع تجزئة كتلة حديثة من دفتر الأستاذ (ledger)، والجدول الزمني للرسوم الذي يُمكن إستخدامه لحساب تكلفة تقديم مُعاملة تستخدمه.

#### المُعلمات (parameters):

- `<object>` - (إختياري) الإلتزام [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### النتائج:

Rpcresponse يحتوي على كائن JSON يتكون من كائن JSON لسلسلة تجزئة الكُتلة (Blockhash) وكائن حاسبة رسوم JSON أو FeeCalculator JSON.

- `RpcResponse<object>` - كائن هيكل إستجابة JSON مع قِيمة `value` تعيين الحقل إلى كائن JSON بما في ذلك:
- تجزئة الكُتلة `blockhash: <string>` - تجزئة الكُتلة كسلسلة مُرمّزة base-58
- حاسبة الرسوم `feeCalculator: <object>` - كائن حاسبة الرسوم (FeeCalculator)، جدول رسوم لتجزئة الكتلة (blockhash) هذه

#### مثال:

الطلب:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getGenesisHash"}
'
```

النتيجة:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1
    },
    "value": {
      "blockhash": "CSymwgTNX1j3E4qhKfJAUE41nBWEwXufoYryPbkde5RR",
      "feeCalculator": {
        "lamportsPerSignature": 5000
      },
      "lastValidSlot": 297
    }
  },
  "id": 1
}
```

### الحصول على نماذج الأداء الحديثة (getRecentPerformanceSamples)

يُرجع قائمة بعينات الأداء الأخيرة، في ترتيب الفُتحة (slot) العكسية. تُؤخذ عينات الأداء كل 60 ثانية وتشمل عدد المُعاملات و الفُتحات (slots) التي تحدث في نافذة زمنية مُعَيَّنة.

#### المُعلمات (parameters):

- الحد الأقصى `limit: <usize>` - (إختياري) عدد العينات المُراد إرجاعها (حد أقصى 720)

#### النتائج:

مجموعة من:

- `RpcPerfSample<object>`
  - `slot: <u64>` - الفُتحة (Slot) التي تم أخذ العينة فيها
  - `numTransactions: <u64>` - عدد المُعاملات في العينة
  - `numTransactions: <u64>` - عدد الفُتحات (Slots) في العينة
  - `samplePeriodSecs: <u16>` - عدد الثواني في نافذة العينة

#### مثال:

الطلب:

```bash
// curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getBlockCommitment","params":[4]}
'
```

النتيجة:

```json
{
  "jsonrpc": "2.0",
  "result": [
    {
      "numSlots": 126,
      "numTransactions": 126,
      "samplePeriodSecs": 60,
      "slot": 348125
    },
    {
      "numSlots": 126,
      "numTransactions": 126,
      "samplePeriodSecs": 60,
      "slot": 347999
    },
    {
      "numSlots": 125,
      "numTransactions": 125,
      "samplePeriodSecs": 60,
      "slot": 347873
    },
    {
      "numSlots": 125,
      "numTransactions": 125,
      "samplePeriodSecs": 60,
      "slot": 347748
    }
  ],
  "id": 1
}
```

### الحصول على لقطة الفُتحة (getSnapshotSlot)

تُرجع أعلى فُتحة (slot) تحتوي العُقدة (node) التي لديها لقطة لها

#### المُعلمات (parameters):

لا شيء

#### النتائج:

- `<u64>` - إلتقاط لقطة للفُتحة (Slot)

#### مثال:

الطلب:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getFees"}
'
```

النتيجة:

```json
{ "jsonrpc": "2.0", "result": 100, "id": 1 }
```

النتيجة عندما لا تحتوي العُقدة (node) على لقطة:

```json
{
  "jsonrpc": "2.0",
  "error": { "code": -32008, "message": "No snapshot" },
  "id": 1
}
```

### الحصول على حالات التوقيع (getSignatureStatuses)

يُرجع حالات لقائمة التوقيعات. ما لم يتم تضمين إعدادات مُعلِّمة البحث في سِجِل المُعاملات (parameter) `searchTransactionHistory` فإن هذه الطريقة تبحث فقط في ذاكرة التخزين المُؤق للحالة الأخيرة للتوقيعات، والتي تحتفظ بالحالات لجميع الفُتحات (slots) النَشِطة بالإضافة إلى تجزئة الحد الأقصى للكتلة الأخيرة `MAX_RECENT_BLOCKHASHES` لفُتحات الجذر (rooted slots).

#### المُعلمات (parameters):

- `<array>` - مجموعة من توقيعات المُعاملات للتأكيد، كسلسلة مُرمّزة base-58
- `<object>` - كائن تكوين (إختياري) يحتوي على الحقول الإختيارية التالية:
  - البحث في سِجِل المعاملات `searchTransactionHistory: <bool>` - إذا كان صحيحا، ستبحث عُقدة Solana في ذاكرة التخزين المُؤقت في دفتر الأستاذ (ledger) عن أي توقيعات غير موجودة في ذاكرة التخزين المُؤقت للحالة الأخيرة

#### النتائج:

Rpcresponse يحتوي على كائن JSON يتألف من مجموعة من عناصر حالة المُعاملة (TransactionStatus).

- `RpcResponse<object>` - كائن هيكل إستجابة الـ Rpc مع حقل بقِيمة `value`:

مجموعة من:

- `<null>` - مُعاملة غير معروفة
- `<object>`
  - `slot: <u64>` - الفُتحة (slot) التي تمت مُعالجة المُعاملة بها
  - التأكيدات `confirmations: <usize | null>` - عدد الكتل (blocks) منذ تأكيد التوقيع، لاغية إذا كانت متجذرة، وكذلك الإنتهاء منها من قبل الأغلبية العُظمى (supermajority) من المجموعة (cluster)
  - `err: <object | null>` - خطأ إذا فشلت المُعاملة، لاغ إذا نجحت المُعاملة. تعريفات خطأ المُعاملة [TransactionError definitions](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
  - حالة التأكيد `confirmationStatus: <string | null>` - حالة تأكيد المُعاملة الخاصة بالمجموعة (cluster)؛ إما تمت مُعالجتها `processed` أو تم تأكيدها `confirmed` أو تم الإنتهاء منها `finalized`. راجع الإتزام [Commitment](jsonrpc-api.md#configuring-state-commitment) للمزيد حول التأكيدات المُتفائلة.
  - إهمال: `status: <object>` - حالة المُعاملة
    - `"Ok": <null>` - المُعاملة كانت ناجحة
    - `"err": <ERR>` - فشلت المُعاملة مع خطأ في المُعاملة (TransactionError)

#### مثال:

الطلب:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getSignatureStatuses",
    "params": [
      [
        "5VERv8NMvzbJMEkV8xnrLkEaWRtSz9CosKDYjCJjBRnbJLgp8uirBgmQpjKhoR4tjF3ZpRzrFmBV6UjKdiSZkQUW",
        "5j7s6NiJS3JAkvgkoc18WVAsiSaci2pxB2A6ueCJP4tprA2TFg9wSyTLeYouxPBJEMzJinENTkpA52YStRW5Dia7"
      ]
    ]
  }
'
```

النتيجة:

```json
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
        },
        "confirmationStatus": "confirmed"
      },
      null
    ]
  },
  "id": 1
}
```

#### مثال:

الطلب:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getBalance",
    "params": [
      "83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri",
      {
        "commitment": "max"
      }
    ]
  }
'
```

النتيجة:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 82
    },
    "value": [
      {
        "slot": 48,
        "confirmations": null,
        "err": null,
        "status": {
          "Ok": null
        },
        "confirmationStatus": "finalized"
      },
      null
    ]
  },
  "id": 1
}
```

### الحصول على الفُتحة (getSlot)

يُرجع الفُتحة (lot) الحالية التي تُعالجها العُقدة (node)

#### المُعلمات (parameters):

- `<object>` (إختياري) الإلتزام [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### النتائج:

- `<u64>` - الفُتحة (slot) الحالية

#### مثال:

الطلب:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getFees"}
'
```

النتيجة:

```json
{ "jsonrpc": "2.0", "result": 1234, "id": 1 }
```

### الحصول على قائد الفُتحة (getSlotLeader)

يُرجع قائد الفُتحة (slot leader) الحالية

#### المُعلمات (parameters):

- `<object>` (إختياري) الإلتزام [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### النتائج:

- `pubkey: <string>` - المفتاح العمومي (Pubkey) لهوية العُقدة (node) كسلسلة مُرمّزة base-58

#### مثال:

الطلب:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getFees"}
'
```

النتيجة:

```json
{
  "jsonrpc": "2.0",
  "result": "ENvAW7JScgYq6o4zKZwewtkzzJgDzuJAFxYasvmEQdpS",
  "id": 1
}
```

### الحصول على تنشيط الحِصَّة (getStakeActivation)

يقوم بإرجاع معلومات تنشيط الفترة (epoch) لحساب الحِصَة (stake)

#### المُعلمات (parameters):

- `<string>` - المفتاح العمومي (pubkey) لحساب التَّحْصِيص (Stake account) الذي سيتم الإستفسار عنه، كسلسلة مُرمّزة base-58
- `<object>` - (إختياري) تحتوي إعدادات الكائن على الحقول الإختيارية التالية:
  - (إختياري) الإلتزام [Commitment](jsonrpc-api.md#configuring-state-commitment)
  - (إختياري) `epoch: <u64>` - الفترة (epoch) التي يُمكن حساب تفاصيل التفعيل لها. إذا لم يتم توفير المُعلِّمة (parameter)، فإن الإفتراضات (Defaults) للفترة (epoch) الحالية.

#### النتائج:

سيكون ناتج الإستجابة (response output) كائن Json مع الحقول التالية:

- `state: <string` - حالة تفعيل حساب التَّحْصِيص (Stake account)، واحدة من: نَشِطة `active`، غير نَشِطة `inactive`، في طور التنشِيط `activating`، في طور إلغاء التنشِيط `deactivating`
- `active: <u64>` - الحِصَّة (stake) نشِطة أثناء الفترة (epoch)
- `inactive: <u64>` - الحِصَّة (stake) غير النشِطة خلال الفترة (epoch)

#### مثال:

الطلب:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getBalance", "params":["83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri"]}
'
```

النتيجة:

```json
{
  "jsonrpc": "2.0",
  "result": { "active": 197717120, "inactive": 0, "state": "active" },
  "id": 1
}
```

#### مثال:

الطلب:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getAccountInfo",
    "params": [
      "4fYNw3dojWmQ4dXtSGE9epjRGy9pFSx62YypT7avPYvA",
      {
        "encoding": "jsonParsed"
      }
    ]
  }
'
```

النتيجة:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "active": 124429280,
    "inactive": 73287840,
    "state": "activating"
  },
  "id": 1
}
```

### الحصول على المعروض (getSupply)

يُرجع معلومات حول المعروض الحالي.

#### المُعلمات (parameters):

- `<object>` (إختياري) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### النتائج:

ستكون النتيجة كائن RpcResponse JSON بقِيمة `value` تُساوي كائن JSON يحتوي على:

- `total: <u64>` - إجمالي المعروض حسب وِحدة الـ lamports
- `circulating: <u64>` - المعروض المُتداول حسب وِحدة الـ lamports
- `nonCirculating: <u64>` - المعروض الغير مُمتداول حسب وِحدة الـ lamports
- `nonCirculatingAccounts: <array>` - مجموعة من عناوين الحساب للحسابات غير المُتداولة، كسلاسل

#### مثال:

الطلب:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getFees"}
'
```

النتيجة:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1114
    },
    "value": {
      "circulating": 16000,
      "nonCirculating": 1000000,
      "nonCirculatingAccounts": [
        "FEy8pTbP5fEoqMV1GdTz83byuA8EKByqYat1PKDgVAq5",
        "9huDUZfxoJ7wGMTffUE7vh1xePqef7gyrLJu9NApncqA",
        "3mi1GmwEE3zo2jmfDuzvjSX9ovRXsDUKHvsntpkhuLJ9",
        "BYxEJTDerkaRWBem3XgnVcdhppktBXa2HbkHPKj2Ui4Z"
      ],
      "total": 1016000
    }
  },
  "id": 1
}
```

### الحصول رصيد رموز الحساب (getTokenAccountBalance)

يُرجع رصيد الرموز لحِساب رمز بصيغة SPL. غير مستقر **UNSTABLE**

#### المُعلمات (parameters):

- `<string>` - المفتاح العمومي (pubkey) للحِساب الذي سيتم الإستفسار عنه، كسلسلة مُرمّزة base-58
- (إختياري) الإلتزام `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### النتائج:

ستكون النتيجة كائن RpcResponse JSON بقِيمة `value` تُساوي كائن JSON يحتوي على:

- `uiAmount: <f64>` - الرصيد بإستخدام الكسور العشرية المُحَدَّدة مُسبقا في عملية السك (Mint)
- `amount: <string>` - الرصيد الخام بدون الكسور العشرية، عرض سلسلة من u64
- `decimals: <u8>` - عدد الأرقام الأساسية 10 إلى يمين الفاصلة العشرية

#### مثال:

الطلب:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getBalance", "params":["83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri"]}
'
```

النتيجة:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1114
    },
    "value": {
      "uiAmount": 98.64,
      "amount": "9864",
      "decimals": 2
    },
    "id": 1
  }
}
```

### الحصول على رمز الحِسابات حسب التفويض (getTokenAccountsByDelegate)

يُرجع جميع حِسابات الرمز بصيغة SPL من قبل المُفوض (Delegate) المُعتمد. غير مُستقر **UNSTABLE**

#### المُعلمات (parameters):

- `<string>` - المفتاح العمومي (pubkey) للحِساب بمُفوض الحِساب الذي سيتم الإستفسار عنه، كسلسلة مُرمّزة base-58
- `<object>` - إما:
  - `mint: <string>` - المفتاح العمومي (Pubkey) للعملة المُحَدَّدة التي تم سَكُّها (Mint) لتقييد الحِسابات، كسلسلة مُرمّزة base-58؛ أو
  - `programId: <string>` - مُعرف رمز البرنامج (Token Program ID) الذي يمتلك الحسابات، كسلسلة مُرمّزة base-58
- `<object>` - (إختياري) تحتوي إعدادات الكائن على الحقول الإختيارية التالية:
  - (إختياري) الإلتزام [Commitment](jsonrpc-api.md#configuring-state-commitment)
  - الترميز `encoding: <string>` - الترميز لبيانات الحساب، إما "base58" بطيء "(_slow_) ، "base64", "base64+zstd" ، أو "jsonParsed". يُحاول ترميز "jsonParsed" إستخدام مُوزعي الحالة الخاصين بالبرنامج لإرجاع المزيد من بيانات حالة الحساب التي يُمكن قراءتها بشكل واضح. إذا طُلب "jsonParsed" ولكن لا يُمكن العثور على عملية سك (mint) صحيحة لحساب مُعَيَّن، فسيتم تصفية هذا الحِساب من النتائج.
  - (إختياري) `dataSlice: <object>` - الحد من بيانات الحساب التي تم إرجاعها بإستخدام `offset: <usize>` المُقدمة و `length: <usize>` الحقول؛ مُتاح فقط لترميز "base58", "base64" أو "base64+zstd".

#### النتائج:

ستكون النتيجة كائن RpcResponse JSON بقِيمة `value` تُساوي مصفوفة من كائنات JSON، والتي ستحتوي على:

- `pubkey: <string>` - المفتاح العمومي (public key) كسلسلة مُرمّزة base-58
- `account: <object>` - كائن JSON، مع الحقول الفرعية التالية:
  - `lamports: <u64>`، عدد الـ lamports المُخَصَّصَة لهذا الحساب، كـ u64
  - المالك `owner: <string>`، المفتاح العمومي (pubkey) المُرمّز base-58 من البرنامج الذي تم تعيين هذا الحساب له
  - `data: <object>`، بيانات حالة الرمز المُرتبطة بالحساب، إما كبيانات ثنائية مُشفَّرة أو بتنسيق `{<program>: <state>}`
  - قابل للتنفيذ `executable: <bool>`، المنطقية (boolean) تُشير إلى ما إذا كان الحساب يحتوي على برنامج \(وهو فقط-للقراءة\)
  - فترة الإيجار `rentEpoch: <u64>`، الفترة (epoch) التي سيكون فيها هذا الحساب مدينا بالإيجار القادم، مثل u64

#### مثال:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getTokenAccountsByDelegate",
    "params": [
      "4Nd1mBQtrMJVYVfKf2PJy9NZUZdTAsp7D4xWLs4gDB4T",
      {
        "programId": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
      },
      {
        "encoding": "jsonParsed"
      }
    ]
  }
'
```

النتيجة:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1114
    },
    "value": [
      {
        "data": {
          "program": "spl-token",
          "parsed": {
            "accountType": "account",
            "info": {
              "tokenAmount": {
                "amount": "1",
                "uiAmount": 0.1,
                "decimals": 1
              },
              "delegate": "4Nd1mBQtrMJVYVfKf2PJy9NZUZdTAsp7D4xWLs4gDB4T",
              "delegatedAmount": 1,
              "isInitialized": true,
              "isNative": false,
              "mint": "3wyAj7Rt1TWVPZVteFJPLa26JmLvdb1CAKEFZm3NY75E",
              "owner": "CnPoSPKXu7wJqxe59Fs72tkBeALovhsCxYeFwPCQH9TD"
            }
          }
        },
        "executable": false,
        "lamports": 1726080,
        "owner": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
        "rentEpoch": 4
      }
    ]
  },
  "id": 1
}
```

### الحصول على رمز الحِسابات حسب المالك (getTokenAccountsByOwner)

إرجاع جميع حِسابات الرمز بصيغة SPL من قبل مالك الرمز. غير مُستقر **UNSTABLE**

#### المُعلمات (parameters):

- `<string>` - المفتاح العمومي (pubkey) للحِساب الذي سيتم الإستفسار عنه، كسلسلة مُرمّزة base-58
- `<object>` - إما:
  - `mint: <string>` - المفتاح العمومي (Pubkey) للعملة المُحَدَّدة التي تم سَكُّها (Mint) للحد من الحِسابات عليه، كسلسلة مُرمّزة base-58؛ أو
  - مُعرف البرنامج `programId: <string>` - المفتاح العمومي (Pubkey) لمُعرف رمز البرنامج (Token Program ID) الذي يمتلك الحِسابات، كسلسلة مُرمّزة base-58
- `<object>` - (إختياري) تحتوي إعدادات الكائن على الحقول الإختيارية التالية:
  - (إختياري) الإلتزام [Commitment](jsonrpc-api.md#configuring-state-commitment)
  - الترميز `encoding: <string>` - الترميز لبيانات الحساب، إما "base58" بطيء "(_slow_) ، "base64", "base64+zstd" ، أو "jsonParsed". يُحاول ترميز "jsonParsed" إستخدام مُوزعي الحالة الخاصين بالبرنامج لإرجاع المزيد من بيانات حالة الحساب التي يُمكن قراءتها بشكل واضح. إذا طُلب "jsonParsed" ولكن لا يُمكن العثور على عملية سَكّ (mint) صحيحة لحِساب مُعَيَّن، فسيتم تصفية هذا الحِساب من النتائج.
  - (إختياري) `dataSlice: <object>` - الحد من بيانات الحِساب التي تم إرجاعها بإستخدام `offset: <usize>` المُقدمة و `length: <usize>` الحقول؛ متاح فقط لترميز "base58", "base64" أو "base64+zstd".

#### النتائج:

ستكون النتيجة كائن RpcResponse JSON بقِيمة `value` تُساوي مصفوفة من كائنات JSON، والتي ستحتوي على:

- `pubkey: <string>` - المفتاح العمومي (public key) كسلسلة مُرمّزة base-58
- `account: <object>` - كائن JSON، مع الحقول الفرعية التالية:
  - `الlamports: <u64>`عدد الـ lamports المُخَصَّصَة لهذا الحساب، كـ u64
  - `المالك: <string>`، المفتاح العمومي (pubkey) المُرمّز base-58 من البرنامج الذي تم تعيين هذا الحساب له
  - `data: <object>`، بيانات حالة الرمز المُرتبطة بالحساب، إما كبيانات ثنائية مُشفَّرة أو بتنسيق `{<program>: <state>}`
  - قابل للتنفيذ `executable: <bool>`، المنطقية (boolean) تُشير إلى ما إذا كان الحِساب يحتوي على برنامج \(وهو فقط-للقراءة\)
  - إيجار الفترة `rentEpoch: <u64>`، الفترة (epoch) التي سيكون فيها هذا الحساب مدينا بالإيجار القادم، ك u64

#### مثال:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getTokenAccountsByOwner",
    "params": [
      "4Qkev8aNZcqFNSRhQzwyLMFSsi94jHqE8WNVTJzTP99F",
      {
        "mint": "3wyAj7Rt1TWVPZVteFJPLa26JmLvdb1CAKEFZm3NY75E"
      },
      {
        "encoding": "jsonParsed"
      }
    ]
  }
'
```

النتيجة:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1114
    },
    "value": [
      {
        "data": {
          "program": "spl-token",
          "parsed": {
            "accountType": "account",
            "info": {
              "tokenAmount": {
                "amount": "1",
                "uiAmount": 0.1,
                "decimals": 1
              },
              "delegate": null,
              "delegatedAmount": 1,
              "isInitialized": true,
              "isNative": false,
              "mint": "3wyAj7Rt1TWVPZVteFJPLa26JmLvdb1CAKEFZm3NY75E",
              "owner": "4Qkev8aNZcqFNSRhQzwyLMFSsi94jHqE8WNVTJzTP99F"
            }
          }
        },
        "executable": false,
        "lamports": 1726080,
        "owner": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
        "rentEpoch": 4
      }
    ]
  },
  "id": 1
}
```

### الحصول على رمز أكبر الحِسابات (getTokenLargestAccounts)

يُرجع أكبر 20 حسابًا بصيغة رمز SPL. غير مُستقر **UNSTABLE**

#### المُعلمات (parameters):

- `<string>` - المفتاح العمومي (pubkey) للعملة التي سيتم سَكُّها (mint) والتي سيتم الإستفسار عنها، كسلسلة مُرمّزة base-58
- `<object>` - (إختياري) الإلتزام [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### النتائج:

ستكون النتيجة كائن RpcResponse JSON بقِيمة `value` تُساوي مصفوفة من كائنات JSON تحتوي على:

- العنوان `address: <string>`، عنوان حساب الرمز
- `uiAmount: <f64>` - رصيد حساب الرمز، بإستخدام الكسور العشرية بإستخدام معايير السَّك (mint) المُسبقة
- `amount: <string>` - الرصيد الخام بدون الكسور العشرية، عرض سلسلة من u64
- الكسور العشرية `decimals: <u8>` - عدد الأرقام الأساسية 10 إلى يمين الفاصلة العشرية

#### مثال:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getTokenLargestAccounts", "params": ["3wyAj7Rt1TWVPZVteFJPLa26JmLvdb1CAKEFZm3NY75E"]}
'
```

النتيجة:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1114
    },
    "value": [
      {
        "address": "FYjHNoFtSQ5uijKrZFyYAxvEr87hsKXkXcxkcmkBAf4r",
        "amount": "771",
        "decimals": 2,
        "uiAmount": 7.71
      },
      {
        "address": "BnsywxTcaYeNUtzrPxQUvzAWxfzZe3ZLUJ4wMMuLESnu",
        "amount": "229",
        "decimals": 2,
        "uiAmount": 2.29
      }
    ]
  },
  "id": 1
}
```

### الحصول على رمز المعروض (getTokenSupply)

يُرجع العرض الإجمالي لنوع الرمز بصيغة SPL. غير مُستقر **UNSTABLE**

#### المُعلمات (parameters):

- `<string>` - المفتاح العمومي (pubkey) للعملة التي سيتم سَكُّها (mint) والتي سيتم الإستفسار عنها، كسلسلة مُرمّزة base-58
- `<object>` - (إختياري) الإلتزام [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### النتائج:

ستكون النتيجة كائن RpcResponse JSON بقِيمة `value` تُساوي كائن JSON يحتوي على:

- `uiAmount: <f64>` - إجمالي المعروض من الرموز بإستخدام الكسور العشرية المحددة مسبقا في عملية السَّك (Mint)
- `amount: <string>` - الرصيد الخام بدون الكسور العشرية، عرض سلسلة من u64
- `decimals: <u8>` - عدد الأرقام الأساسية 10 إلى يمين الفاصلة العشرية

#### مثال:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getTokenSupply", "params": ["3wyAj7Rt1TWVPZVteFJPLa26JmLvdb1CAKEFZm3NY75E"]}
'
```

النتيجة:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1114
    },
    "value": {
      "uiAmount": 1000,
      "amount": "100000",
      "decimals": 2
    }
  },
  "id": 1
}
```

### الحصول على عدد المُعاملات (getTransactionCount)

Ledger عدد المُعاملات الحالية من دفتر الأستاذ (ledger)

#### المُعلمات (parameters):

- `<object>` - (إختياري) الإلتزام [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### النتائج:

- `<u64>` - العد (count)

#### مثال:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getLargestAccounts"}
'

```

النتيجة:

```json
{ "jsonrpc": "2.0", "result": 268, "id": 1 }
```

### الحصول على الإصدار (getVersion)

يُرجع إصدارات solana الحالية قيد التشغيل على العُقدة (node)

#### المُعلمات (parameters):

لا شيء

#### النتائج:

سيكون ناتج الإستجابة (response output) كائن Json مع الحقول التالية:

- `solana-core`، نُسخة البرنامج من Solana-core
- `feature-set`، مُعرف فريد لمجموعة ميزة البرنامج الحالي

#### مثال:

الطلب:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getFees"}
'
```

النتيجة:

```json
{ "jsonrpc": "2.0", "result": { "solana-core": "1.6.0" }, "id": 1 }
```

### الحصول على حِسابات التصويت (getVoteAccounts)

يُرجع معلومات الحِساب والحِصَّة (stake) المُرتبطة لجميع حِسابات التصويت (voting accounts) في البنك الحالي.

#### المُعلمات (parameters):

- `<object>` - (إختياري) الإلتزام [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### النتائج:

سيكون حقل النتيجة كائن JSON الحالي `current` و الحسابات المُنحرفة `delinquent`، يحتوي كل منها على مجموعة من كائنات JSON مع الحقول الفرعية التالية:

- `pubkey: <string>` - المفتاح العمومي لحساب التصويت (votePubkey) كسلسلة مُرمّزة base-58
- `nodePubkey: <string>` - المفتاح العمومي للعُقدة (Node public key)، كسلسلة مُرمّزة base-58
- `nodePubkey. Stake: <u64>` - الحِصَّة (stake)، بحساب الوحدة lamports، تم تفويضها إلى حساب التصويت (vote account) هذا ونَشِطة في هذه الفترة (epoch)
- `epochVoteAccount: <bool>` - bool، منطقي ما إذا كان حساب التصويت (vote account) خاضعا لهذه الفترة (epoch)
- `commission: <number>`، النسبة المئوية (0-100) من المُكافآت المدفوعة لحساب التصويت (vote account)
- `lastVote: <u64>` - أحدث فُتحة (slot) تم التصويت عليها بواسطة حساب التصويت (vote account) هذا
- `epochcredits: <array>` - تاريخ عدد الأرصدة (credits) المُكتسبة في نهاية كل فترة (epoch)، كمجموعة من المجموعات تحتوي على: `[epoch, credits, previousCredits]`

#### مثال:

الطلب:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getLargestAccounts"}
'
```

النتيجة:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "current": [
      {
        "commission": 0,
        "epochVoteAccount": true,
        "epochCredits": [
          [1, 64, 0],
          [2, 192, 64]
        ],
        "nodePubkey": "B97CCUW3AEZFGy6uUg6zUdnNYvnVq5VG8PUtb2HayTDD",
        "lastVote": 147,
        "activatedStake": 42,
        "votePubkey": "3ZT31jkAGhUaw8jsy4bTknwBMP8i4Eueh52By4zXcsVw"
      }
    ],
    "delinquent": [
      {
        "commission": 127,
        "epochVoteAccount": false,
        "epochCredits": [],
        "nodePubkey": "6ZPxeQaDo4bkZLRsdNrCzchNQr5LN9QMc9sipXv9Kw8f",
        "lastVote": 0,
        "activatedStake": 0,
        "votePubkey": "CmgCk4aMS7KW1SHX3s9K5tBJ6Yng2LBaC8MFov4wx9sm"
      }
    ]
  },
  "id": 1
}
```

### الحصول على الحد الأدنى لفُتحة دفتر الأستاذ (minimumLedgerSlot)

تُرجع أدنى فُتحة (slot) التي تحتوي العُقدة (node) التي لديها معلومات عن دفتر الأستاذ (ledger) الخاص بها. قد تزيد هذه القيمة بمرور الوقت إذا تم تكوين العُقدة (node) لمسح بيانات دفتر الأستاذ (ledger) القديم

#### المُعلمات (parameters):

لا شيء

#### النتائج:

- `u64` - الحد الأدنى لفُتحة (slot) دفتر الأستاذ (ledger)

#### مثال:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"minimumLedgerSlot"}
'

```

النتيجة:

```json
{ "jsonrpc": "2.0", "result": 1234, "id": 1 }
```

### طلب التوزيع الحر (requestAirdrop)

طلب توزيع الحر (Airdrop) بوحدات lamports إلى مفتاح عمومي (Pubkey)

#### المُعلمات (parameters):

- `<string>` - المفتاح العمومي (pubkey) للحِساب الذي سيتم تلقي lamports عليه، كسلسلة مُرمّزة base-58
- `<integer>` - الـ lamports، كـ u64
- `<object>` - (إختياري) - الإلتزام [Commitment](jsonrpc-api.md#configuring-state-commitment) (يُستخدم لإستعادة تجزئة الكتلة (Blockhash) والتَحَقُّق من نجاح التوزيع الحر)

#### النتائج:

- `<string>` - توقيع توقيع مُعاملة التوزيع الحر (airdrop)، كسلسلة مُرمّزة base-58

#### مثال:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getBalance", "params":["83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri"]}
'

```

النتيجة:

```json
{
  "jsonrpc": "2.0",
  "result": "5VERv8NMvzbJMEkV8xnrLkEaWRtSz9CosKDYjCJjBRnbJLgp8uirBgmQpjKhoR4tjF3ZpRzrFmBV6UjKdiSZkQUW",
  "id": 1
}
```

### إرسال مُعاملة (sendTransaction)

يُرسل مُعاملة مُوقعة إلى المجموعة (cluster) لمُعالجتها.

هذه الطريقة لا تغير مُعاملة بأي شكل من الأشكال؛ فهي ترسل المُعاملة التي أنشأها العملاء إلى العُقدة (node) كما هي.

إذا كانت خدمة rpc للعُقدة (node) تتلقى المُعاملة، فإن هذه الطريقة تنجح على الفور، دون إنتظار أي تأكيدات. لا تضمن الإستجابة الناجحة لهذه الطريقة مُعالجة المُعاملة أو تأكيدها بواسطة المجموعة (cluster).

في حين أن خدمة rpc ستُحاول بشكل معقول إرسالها، فإنه يُمكن رفض المُعاملة إذا إنتهت صلاحية تجزئة الكُتلة الحديثة للمُعاملة `Recent_blockhash` قبل أن تستقر.

إستخدم الحصول على حالات التوقيع [`getSignatureStatuses`](jsonrpc-api.md#getsignaturestatuses) لضمان مُعالجة وتأكيد المُعاملة.

قبل التقديم، تجري عمليات التَحَقُّق التالية:

1. يتم التَحَقُّق من توقيعات المُعاملة
2. تتم مُحاكاة المُعاملة في فُتحة (Slot) البنك المُحَدَّدة بواسطة الإلتزام بالإختبار المبدئي (preflight). عند الفشل سيتم إرجاع خطأ. قد تكون فحوصات الإختبار المبدئي (preflight) مُعطلة إذا رغبت في ذلك. من المُستحسن تحديد الإلتزام نفسه وإلتزام الإختبار المبدئي (preflight) لتجنب السلوك المُربك.

التوقيع المُعاد هو التوقيع الأول في المُعاملة، والذي يُستخدم لتحديد مُعرف المُعاملة ([transaction id](../../terminology.md#transanction-id)). يُمكن بسهولة إستخراج هذا المُعرف من بيانات المُعاملة قبل تقديمه.

#### المُعلمات (parameters):

- `<string>` - المُعاملة المُوقعة بالكامل، كسلسلة مُرمّزة
- `<object>` - (إختياري) إعدادات الكائن تحتوي على الحقول الإختيارية التالية:
  - `skipPreflight: <bool>` - إذا كان صحيحا، تخطي عمليات التحقق قبل الطيران (المُفترض: خطأ)
  - `preflightCommitment: <string>` - (إختياري) [Commitment](jsonrpc-api.md#configuring-state-commitment) لإستخدام الإختبار المبدئي (المُفترض: `"max"`).
  - `encoding: <string>` - الترميز (إختياري) المُستخدم لبيانات المُعاملة. إما `"base58"` (_slow_، **DEPRECATED**) ، أو `"base64"`. (المُفترض: `"base58"`).

#### النتائج:

- `<string>` - أول توقيع مُعاملة مُضمن في المُعاملة، كسلسلة مُرمّزة base-58 لمُعرف المُعاملة ([transaction id](../../terminology.md#transanction-id))

#### مثال:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "sendTransaction",
    "params": [
      "4hXTCkRzt9WyecNzV1XPgCDfGAZzQKNxLXgynz5QDuWWPSAZBZSHptvWRL3BjCvzUXRdKvHL2b7yGrRQcWyaqsaBCncVG7BFggS8w9snUts67BSh3EqKpXLUm5UMHfD7ZBe9GhARjbNQMLJ1QD3Spr6oMTBU6EhdB4RD8CP2xUxr2u3d6fos36PD98XS6oX8TQjLpsMwncs5DAMiD4nNnR8NBfyghGCWvCVifVwvA8B8TJxE1aiyiv2L429BCWfyzAme5sZW8rDb14NeCQHhZbtNqfXhcp2tAnaAT"
    ]
  }
'

```

النتيجة:

```json
{
  "jsonrpc": "2.0",
  "result": "2id3YC2jK9G5Wo2phDx4gJVAew8DcY5NAojnVuao8rkxwPYPe8cSwE5GzhEgJA2y8fVjDEo6iR6ykBvDxrTQrtpb",
  "id": 1
}
```

### مُحاكاة المُعاملة (simulateTransaction)

مُحاكاة إرسال مُعاملة

#### المُعلمات (parameters):

- `<string>` - المُعاملة، كسلسلة مُرمّزة. يجب أن تحتوي المُعاملة على تجزئة كتلة (Blockhash) صالحة، ولكن ليس مطلوبا أن يتم توقيعه.
- `<object>` - كائن تكوين (إختياري) يحتوي على الحقول الإختيارية التالية:
  - التحقق من التوقيع `sigVerify: <bool>` - إذا كان صحيحا سيتم التَحَقُّق من توقيعات المُعاملة (المُفترض: خاطئ)
  - الإتزام `commitment: <string>` - (إختياري) [Commitment](jsonrpc-api.md#configuring-state-commitment) المُستوى لمُحاكاة المُعاملة (المُفترض: `"max"`).
  - `encoding: <string>` - الترميز (إختياري) المُستخدم لبيانات المُعاملة. إما `"base58"` (_slow_، **DEPRECATED**) ، أو `"base64"`. (المُفترض: `"base58"`).

#### النتائج:

يحتوي Rpcresponse على كائن حالة المُعاملة. ستكون النتيجة كائن JSON RpcResponse مع قِيمة `value` مُعَيَّنة إلى كائن JSON مع الحقول التالية:

- `err: <object | string | null>` - خطأ إذا فشلت المُعاملة، لاغِ إذا نجحت المُعاملة. تعريفات خطأ المُعاملة [TransactionError definitions](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
- `logs: <array | null>` - مجموعة من رسائل تسجيل المُعاملات التي يتم إخراجها أثناء التنفيذ، لاغية إذا فشلت المُحاكاة قبل أن تتمكن المُعاملة من التنفيذ (على سبيل المثال بسبب فشل التَحَقُّق من تجزئة الكتلة أو التوقيع غير صحيح)

#### مثال:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "simulateTransaction",
    "params": [
      "4hXTCkRzt9WyecNzV1XPgCDfGAZzQKNxLXgynz5QDuWWPSAZBZSHptvWRL3BjCvzUXRdKvHL2b7yGrRQcWyaqsaBCncVG7BFggS8w9snUts67BSh3EqKpXLUm5UMHfD7ZBe9GhARjbNQMLJ1QD3Spr6oMTBU6EhdB4RD8CP2xUxr2u3d6fos36PD98XS6oX8TQjLpsMwncs5DAMiD4nNnR8NBfyghGCWvCVifVwvA8B8TJxE1aiyiv2L429BCWfyzAme5sZW8rDb14NeCQHhZbtNqfXhcp2tAnaAT"
    ]
  }
'
```

النتيجة:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 218
    },
    "value": {
      "err": null,
      "logs": [
        "BPF program 83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri success"
      ]
    }
  },
  "id": 1
}
```

### تعيين عامل فلترة السِجِل (setLogFilter)

تعيين عامل فلترة السِجِل (log filter) على المُدقّق (validator)

#### المُعلمات (parameters):

- `<string>` - عامل فلترة السِجِل الجديد المُراد إستخدامه

#### النتائج:

- `<null>`

#### مثال:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getBlockCommitment","params":[5]}
'
```

النتيجة:

```json
{ "jsonrpc": "2.0", "result": 500, "id": 1 }
```

### خروج المُدقّق (validatorExit)

إذا قام المُدقّق (validator) بالإشتغال مع تفعيل خاصية خروج RPC (مُعلِّمة `--enable-rpc-exit`)، فسيُؤدي هذا الطلب إلى خروج المُدقّق (validator).

#### المُعلمات (parameters):

لا شيء

#### النتائج:

- `<bool>` - ما إذا كانت عملية خروج المُدقّق (validator) ناجحة

#### مثال:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getIdentity"}
'

```

النتيجة:

```json
{ "jsonrpc": "2.0", "result": 500, "id": 1 }
```

## إشتراك الWebsocket أو Subscription Websocket

بعد الإتصال بمِقبَس واب RPC PubSub على `ws://<ADDRESS>/`:

- أرسل طلبات الإشتراك إلى موقع الـ websocket بإستخدام الأساليب أدناه
- قد تكون الإشتراكات المُتعددة نشطة في وقت واحد
- تأخذ العديد من الإشتراكات الخيار الإختياري الإلتزام [commitment`` parameter](jsonrpc-api.md#configuring-state-commitment)، وتحدد كيفية الإنتهاء من التغيير لإطلاق إشعار. بالنسبة للإشتراكات، إذا كان الإلتزام غير مُحَدَّدَ، القيمة المُفترضة هي `"singleGossip"`.

### الإشتراك في الحِساب (accountSubscribe)

إشترك في حِساب لتلقي إشعارات عندما تتغير الـ lamports أو بيانات المفتاح العمومي (public key) لحِساب ما

#### المُعلمات (parameters):

- `<string>` - المفتاح العمومي (pubkey)، كسلسلة مُرمّزة base-58
- `<object>` - (إختياري) تحتوي إعدادات الكائن على الحقول الإختيارية التالية:
  - `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)
  - الترميز `encoding: <string>` - الترميز لبيانات الحساب، إما "base58" بطيء "(_slow_) ، "base64", "base64+zstd" ، أو "jsonParsed". يُحاول ترميز "jsonParsed" إستخدام موزعي الحالة الخاصين بالبرنامج لإرجاع المزيد من بيانات حالة الحساب التي يمكن قراءتها بشكل واضح. إذا طُلب "jsonParsed" ولكن لا يمكن العثور على مُحلل (parser)، يعود الحقل إلى الترميز ثنائي، يمكن الكشف عنها عندما تكون `data` الحقل من نوع`<string>`.

#### النتائج:

- `<number>` - مُعرف الإشتراك \(مطلوب لإلغاء الإشتراك\)

#### مثال:

الطلب:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "accountSubscribe",
  "params": [
    "CM78CPUeXjn8o3yroDHxUtKsZZgoy4GPkPPXfouKNH12",
    {
      "encoding": "base64",
      "commitment": "root"
    }
  ]
}
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "accountSubscribe",
  "params": [
    "CM78CPUeXjn8o3yroDHxUtKsZZgoy4GPkPPXfouKNH12",
    {
      "encoding": "jsonParsed"
    }
  ]
}
```

النتيجة:

```json
{ "jsonrpc": "2.0", "result": 23784, "id": 1 }
```

#### تنسيق الإشعار:

ترميز Base58:

```json
{
  "jsonrpc": "635.0",
  "result": {
    "context": {
      "slot": 52378499307
    },
    "value": {
      "data": [
        "11116bv5nS2h3y12kD1yUKeMZvGcKLSjQgX6BeV7u1FrjeJcKfsHRTPuR3oZ1EioKtYGiYxpxMG5vpbZLsbcBYBEmZZcMKaSoGx9JZeAuWf",
        "base58"
      ],
      "executable": false,
      "lamports": 33594,
      "owner": "11111111111111111111111111111111",
      "rentEpoch": 2
    }
  },
  "id": 1
}
```

ترميز Parsed-JSON:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1
    },
    "value": {
      "data": {
        "nonce": {
          "initialized": {
            "authority": "Bbqg1M4YVVfbhEzwA9SpC9FhsaG83YMTYoR4a8oTDLX",
            "blockhash": "3xLP3jK6dVJwpeGeTDYTwdDK3TKchUf1gYYGHa4sF3XJ",
            "feeCalculator": {
              "lamportsPerSignature": 5000
            }
          }
        }
      },
      "executable": false,
      "lamports": 1000000000,
      "owner": "11111111111111111111111111111111",
      "rentEpoch": 2
    }
  },
  "id": 1
}
```

### إلغاء الإشتراك في الحِساب (accountUnsubscribe)

إلغاء الإشتراك من إشعارات تغيير الحساب

#### المُعلمات (parameters):

- `<number>` - مُعرف إشتراك في الحِساب المُقرر إلغاؤه

#### النتائج:

- `<bool>` - رسالة نجاح إلغاء الإشتراك

#### مثال:

الطلب:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "accountUnsubscribe", "params": [0] }
```

النتيجة:

```json
{ "jsonrpc": "2.0", "result": true, "id": 1 }
```

### سِجِلات الإشتراك (logsSubscribe)

إشترك في تسجيل المعاملات. **UNSTABLE**

#### المُعلمات (parameters):

- الفلتر `filter: <string>|<object>` - معايير الفلترة لسِجِلات تلقي النتائج حسب نوع الحساب؛ مدعومة حاليا:
  - الكل "all" - الإشتراك في جميع المُعاملات بإستثناء مُعاملات التصويت البسيطة
  - الكل مع الأصوات "allWithVotes" - الإشتراك في جميع المُعاملات بما في ذلك مُعاملات التصويت البسيطة
  - `{ "mentions": [ <string> ] }` - إشترك في جميع المُعاملات التي تُشير إلى المفتاح العمومي (pubkey) المُقدم (بوصفها سلسلة مُرمّزة base-58)
- `<object>` - (إختياري) تحتوي إعدادات الكائن على الحقول الإختيارية التالية:
  - (إختياري) الإلتزام [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### النتائج:

- `<integer>` - مُعرف الإشتراك \(مطلوب لإلغاء الإشتراك\)

#### مثال:

طلب:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "logsSubscribe",
  "params": [
    {
      "mentions": [ "11111111111111111111111111111111" ]
    },
    {
      "commitment": "max"
    }
  ]
}
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "logsSubscribe",
  "params": [ "all" ]
}
```

النتيجة:

```json
{ "jsonrpc": "2.0", "result": 24040, "id": 1 }
```

#### تنسيق الإشعار:

ترميز Base58:

```json
{
  "jsonrpc": "2.0",
  "method": "logsNotification",
  "params": {
    "result": {
      "context": {
        "slot": 5208469
      },
      "value": {
        "signature": "5h6xBEauJ3PK6SWCZ1PGjBvj8vDdWG3KpwATGy1ARAXFSDwt8GFXM7W5Ncn16wmqokgpiKRLuS83KUxyZyv2sUYv",
        "err": null,
        "logs": [
          "BPF program 83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri success"
        ]
      }
    },
    "subscription": 24040
  }
}
```

### سِجِلات إلغاء الإشتراك (logsUnsubscribe)

إلغاء الاشتراك في تسجيل المُعاملات

#### المُعلمات (parameters):

- `<integer>` - مُعرف الإشتراك المُقرر إلغاؤه

#### النتائج:

- `<bool>` - رسالة نجاح إلغاء الإشتراك

#### مثال:

الطلب:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "logsUnsubscribe", "params": [0] }
```

النتيجة:

```json
{ "jsonrpc": "2.0", "result": true, "id": 1 }
```

### الإشتراك في البرنامج (programSubscribe)

إشترك في حِساب لتلقي إشعارات عندما تتغير الـ lamports أو البيانات الخاصة بحساب مُعين مملوك من قبل البرنامج

#### المُعلمات (parameters):

- `<string>` - المفتاح العمومي (Pubkey) لمُعرف برنامج، كسلسلة مُرمّزة base-58
- `<object>` - (إختياري) تحتوي إعدادات الكائن على الحقول الإختيارية التالية:
  - (إختياري) - الإلتزام [Commitment](jsonrpc-api.md#configuring-state-commitment)
  - الترميز `encoding: <string>` - الترميز لبيانات الحساب، إما "base58" بطيء "(_slow_) ، "base64", "base64+zstd" ، أو "jsonParsed". يُحاول ترميز "jsonParsed" إستخدام مُوزعي الحالة الخاصين بالبرنامج لإرجاع المزيد من بيانات حالة الحساب التي يُمكن قراءتها بشكل واضح. إذا طُلب "jsonParsed" ولكن لا يُمكن العثور على مُحلِّل (parser)، يعود الحقل إلى الترميز "base64"، يُمكن الكشف عنها عندما تكون بيانات `data` الحقل من النوع`<string>`.
  - (اختياري) `filters: <array>` - نتائج الفلترة بإستخدام كائنات فلترة مُختلفة [filter objects](jsonrpc-api.md#filters); الحساب يجب أن يفي بجميع معايير الفلترة لتضمينها في النتائج

#### النتائج:

- `<integer>` - مُعرف الإشتراك \(مطلوب لإلغاء الإشتراك\)

#### مثال:

الطلب:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "programSubscribe",
  "params": [
    "11111111111111111111111111111111",
    {
      "encoding": "base64",
      "commitment": "max"
    }
  ]
}
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "programSubscribe",
  "params": [
    "11111111111111111111111111111111",
    {
      "encoding": "jsonParsed"
    }
  ]
}
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "programSubscribe",
  "params": [
    "11111111111111111111111111111111",
    {
      "encoding": "base64",
      "filters": [
        {
          "dataSize": 80
        }
      ]
    }
  ]
}
```

النتيجة:

```json
{ "jsonrpc": "2.0", "result": 24040, "id": 1 }
```

#### تنسيق الإشعار:

ترميز Base58:

```json
{
  "jsonrpc": "635.0",
  "result": {
    "context": {
      "slot": 52378499307
    },
    "value": {
      "data": [
        "11116bv5nS2h3y12kD1yUKeMZvGcKLSjQgX6BeV7u1FrjeJcKfsHRTPuR3oZ1EioKtYGiYxpxMG5vpbZLsbcBYBEmZZcMKaSoGx9JZeAuWf",
        "base58"
      ],
      "executable": false,
      "lamports": 33594,
      "owner": "11111111111111111111111111111111",
      "rentEpoch": 2
    }
  },
  "id": 1
}
```

ترميز Parsed-JSON:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1
    },
    "value": {
      "data": {
        "nonce": {
          "initialized": {
            "authority": "Bbqg1M4YVVfbhEzwA9SpC9FhsaG83YMTYoR4a8oTDLX",
            "blockhash": "3xLP3jK6dVJwpeGeTDYTwdDK3TKchUf1gYYGHa4sF3XJ",
            "feeCalculator": {
              "lamportsPerSignature": 5000
            }
          }
        }
      },
      "executable": false,
      "lamports": 1000000000,
      "owner": "11111111111111111111111111111111",
      "rentEpoch": 2
    }
  },
  "id": 1
}
```

### إلغاء الإشتراك في البرنامج (programSubscribe)

إلغاء الاشتراك من إشعارات تغيير الحساب المملوك للبرنامج

#### المُعلمات (parameters):

- `<integer>` - مُعرف إشتراك في الحساب المُقرر إلغاؤه

#### النتائج:

- `<bool>` - رسالة نجاح إلغاء الإشتراك

#### مثال:

الطلب:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "programunsubscribe", "params": [0] }
```

النتيجة:

```json
{ "jsonrpc": "2.0", "result": true, "id": 1 }
```

### الإشتراك في التوقيع (signatureSubscribe)

إشترك في توقيع المُعاملة لتلقي إشعار عندما يتم تأكيد المُعاملة في إشعار التوقيع `signatureNotification`، يتم إلغاء الإشتراك تلقائياً

#### المُعلمات (parameters):

- `<string>` - توقيع المُعاملة، كسلسلة مرمّزة base-58
- `<object>` - (إختياري) الإلتزام [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### النتائج:

- `` - مُعرف الإشتراك \(مطلوب لإلغاء الإشتراك\)

#### مثال:

الطلب:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "signatureSubscribe",
  "params": [
    "2EBVM6cB8vAAD93Ktr6Vd8p67XPbQzCJX47MpReuiCXJAtcjaxpvWpcg9Ege1Nr5Tk3a2GFrByT7WPBjdsTycY9b"
  ]
}

{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "signatureSubscribe",
  "params": [
    "2EBVM6cB8vAAD93Ktr6Vd8p67XPbQzCJX47MpReuiCXJAtcjaxpvWpcg9Ege1Nr5Tk3a2GFrByT7WPBjdsTycY9b",
    {
      "commitment": "max"
    }
  ]
}
```

النتيجة:

```json
{ "jsonrpc": "2.0", "result": 0, "id": 1 }
```

#### تنسيق الإشعار:

```bash
{
  "jsonrpc": "2.0",
  "method": "signatureNotification",
  "params": {
    "result": {
      "context": {
        "slot": 5207624
      },
      "value": {
        "err": null
      }
    },
    "subscription": 24006
  }
}
```

### إلغاء الإشتراك في التوقيع (signatureSubscribe)

إلغاء الاشتراك من إشعار تأكيد التوقيع

#### المُعلمات (parameters):

- `<integer>` - مُعرف الإشتراك المُقرر إلغاؤه

#### النتائج:

- `<bool>` - رسالة نجاح إلغاء الإشتراك

#### مثال:

الطلب:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "signatureUnsubscribe", "params": [0] }
```

النتيجة:

```json
{ "jsonrpc": "2.0", "result": true, "id": 1 }
```

### الإشتراكك في الفُتحة (slotSubscribe)

إشترك لتلقي إشعار في أي وقت يتم مُعالجة فُتحة (slot) من قبل المُدقق (validator)

#### المُعلمات (parameters):

لا شيء

#### النتائج:

- `` - مُعرف الإشتراك \(مطلوب لإلغاء الإشتراك\)

#### مثال:

الطلب:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "slotSubscribe" }
```

النتيجة:

```json
{ "jsonrpc": "2.0", "result": 0, "id": 1 }
```

#### تنسيق الإشعار:

```bash
{
  "jsonrpc": "2.0",
  "method": "slotNotification",
  "params": {
    "result": {
      "parent": 75,
      "root": 44,
      "slot": 76
    },
    "subscription": 0
  }
}
```

### إلغاء الإشتراك في الفتحة (slotUnsubscribe)

إلغاء الاشتراك من إشعارات (slot)

#### المُعلمات (parameters):

- `<integer>` - مُعرف الإشتراك القرر إلغاؤه

#### النتائج:

- `<bool>` - رسالة نجاح إلغاء الإشتراك

#### مثال:

الطلب:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "slotunsubscribe", "params": [0] }
```

النتيجة:

```json
{ "jsonrpc": "2.0", "result": true, "id": 1 }
```

### الإشتراك في الجذر (rooSubscribe)

إشترك لتلقي إشعار في أي وقت يتم تعيين جذر (root) جديد من قبل المُدقّق (validator).

#### المُعلمات (parameters):

لا شيء

#### النتائج:

- `` - مُعرف الإشتراك \(مطلوب لإلغاء الإشتراك\)

#### مثال:

الطلب:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "rootSubscribe" }
```

النتيجة:

```json
{ "jsonrpc": "2.0", "result": 0, "id": 1 }
```

#### تنسيق الإشعار:

والنتيجة هي أحدث رقم لفتحة الجذر (root slot).

```bash
{
  "jsonrpc": "2.0",
  "method": "rootNotification",
  "params": {
    "result": 42,
    "subscription": 0
  }
}
```

### إلغاء الإشتراك في الجذر (rootUnsubscribe)

إلغاء الإشتراك من إشعارات الجذر (root)

#### المُعلمات (parameters):

- `<integer>` - مُعرف الإشتراك المُقرر إلغاؤه

#### النتائج:

- `<bool>` - رسالة نجاح إلغاء الإشتراك

#### مثال:

الطلب:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "rootUnsubscribe", "params": [0] }
```

النتيجة:

```json
{ "jsonrpc": "2.0", "result": true, "id": 1 }
```

### الإشتراك في التصويت (voteSubscribe) - غير مستقر، المُفترض (Default) مُعطل

**هذا الإشتراك غير مُستقر ومُتاح فقط إذا تم تشغيل المُدقّق (validator) بالعلامة `--rpc-pubsubenable-vote-subscrative`. تنسيق هذا الإشتراك قد يتغير في المُستقبل**

إشترك لتلقي الإشعار في أي وقت يتم فيه مُراقبة تصويت جديد في أصوات القيل والقال (gossip). هذه الأصوات مُسبقة الإجماع (pre-consensus) لذلك لا يُوجد ضمان لهذه الأصوات لدخول دفتر الأستاذ (ledger).

#### المُعلمات (parameters):

لا شيء

#### النتائج:

- `` - مُعرف الإشتراك \(مطلوب لإلغاء الإشتراك\)

#### مثال:

الطلب:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "voteSubsubscribe" }
```

النتيجة:

```json
{ "jsonrpc": "2.0", "result": 0, "id": 1 }
```

#### تنسيق الإشعار:

النتيجة هي أحدث الأصوات، التي تحتوي على هشاشة، وقائمة بالفُتحات المُصَوِّتَة (voted slots) وختم زمني (timestamp) إختياري.

```json
{
  "jsonrpc": "2.0",
  "method": "voteNotification",
  "params": {
    "result": {
      "hash": "8Rshv2oMkPu5E4opXTRyuyBeZBqQ4S477VG26wUTFxUM",
      "slots": [1, 2],
      "timestamp": null
    },
    "subscription": 0
  }
}
```

### إلغاء الإشتراك في التصويت (voteUnsubscribe)

إلغاء الإشتراك من إشعارات التصويت

#### المُعلمات (parameters):

- `<integer>` - مُعرف الإشتراك المُقرر إلغاؤه

#### النتائج:

- `<bool>` - رسالة نجاح إلغاء الإشتراك

#### مثال:

الطلب:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "voteunsubscribe", "params": [0] }
```

الإستجابة:

```json
{ "jsonrpc": "2.0", "result": true, "id": 1 }
```
