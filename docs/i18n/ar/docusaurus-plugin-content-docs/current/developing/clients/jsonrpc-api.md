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
- [getBlock](jsonrpc-api.md#getblock)
- [getBlockProduction](jsonrpc-api.md#getblockproduction)
- [الحصول على إلتزام الكتلة (getBlockCommitment)](jsonrpc-api.md#getblockcommitment)
- [getBlocks](jsonrpc-api.md#getblocks)
- [getBlocksWithLimit](jsonrpc-api.md#getblockswithlimit)
- [الحصول على وقت الكتلة (getBlockTime)](jsonrpc-api.md#getblocktime)
- [الحصول على عُقَد المجموعة (getClusterNodes)](jsonrpc-api.md#getclusternodes)
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
- [getInflationReward](jsonrpc-api.md#getinflationreward)
- [الحصول على أكبر الحسابات (getLargestAccounts)](jsonrpc-api.md#getlargestaccounts)
- [الحصول على جدول القائد (getLeaderSchedule)](jsonrpc-api.md#getleaderschedule)
- [getMaxRetransmitSlot](jsonrpc-api.md#getmaxretransmitslot)
- [getMaxShredInsertSlot](jsonrpc-api.md#getmaxshredinsertslot)
- [الحصول على الحد الأدنى للرصيد للإعفاء من الإيجار (getMinimumBalanceForRentExemption)](jsonrpc-api.md#getminimumbalanceforrentexemption)
- [الحصول على حسابات مُتعددة (getMultipleAccounts)](jsonrpc-api.md#getmultipleaccounts)
- [الحصول على حسابات البرنامج (getProgramAccounts)](jsonrpc-api.md#getprogramaccounts)
- [الحصول على تجزئة الكتلة الحديثة (getRecentBlockhash)](jsonrpc-api.md#getrecentblockhash)
- [الحصول على نماذج الأداء الحديثة (getRecentPerformanceSamples)](jsonrpc-api.md#getrecentperformancesamples)
- [getSignaturesForAddress](jsonrpc-api.md#getsignaturesforaddress)
- [الحصول على حالات التوقيع (getSignatureStatuses)](jsonrpc-api.md#getsignaturestatuses)
- [الحصول على الفُتحة (getSlot)](jsonrpc-api.md#getslot)
- [الحصول على قائد الفُتحة (getSlotLeader)](jsonrpc-api.md#getslotleader)
- [getSlotLeaders](jsonrpc-api.md#getslotleaders)
- [الحصول على تنشيط الحِصَّة (getStakeActivation)](jsonrpc-api.md#getstakeactivation)
- [الحصول على المعروض (getSupply)](jsonrpc-api.md#getsupply)
- [الحصول على رصيد حِساب الرموز (getTokenAccountBalance)](jsonrpc-api.md#gettokenaccountbalance)
- [الحصول على حِسابات الرموز حسب التفويض (getTokenAccountsByDelegate)](jsonrpc-api.md#gettokenaccountsbydelegate)
- [الحصول على حِسابات الرموز حسب المالك (getTokenAccountsByOwner)](jsonrpc-api.md#gettokenaccountsbyowner)
- [الحصول على أكبر حِسابات الرموز (getTokenLargestAccounts)](jsonrpc-api.md#gettokenlargestaccounts)
- [الحصول على معروض الرمز (getTokenSupply)](jsonrpc-api.md#gettokensupply)
- [getTransaction](jsonrpc-api.md#gettransaction)
- [الحصول على عدد المُعاملات (getTransactionCount)](jsonrpc-api.md#gettransactioncount)
- [الحصول على الإصدار (getVersion)](jsonrpc-api.md#getversion)
- [الحصول على حسابات التصويت (getVoteAccounts)](jsonrpc-api.md#getvoteaccounts)
- [الحصول على الحد الأدنى لفُتحة دفتر الأستاذ (minimumLedgerSlot)](jsonrpc-api.md#minimumledgerslot)
- [طلب التوزيع الحر (requestAirdrop)](jsonrpc-api.md#requestairdrop)
- [إرسال المُعاملة (sendTransaction)](jsonrpc-api.md#sendtransaction)
- [مُحاكاة المُعاملة (simulateTransaction)](jsonrpc-api.md#simulatetransaction)
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

### Deprecated Methods

- [الحصول على الكتلة المُؤَكدة (getConfirmedBlock)](jsonrpc-api.md#getconfirmedblock)
- [الحصول على الكتل المُؤَكدة (getConfirmedBlocks)](jsonrpc-api.md#getconfirmedblocks)
- [الحصول على الكتل المُؤَكدة بحدود (getConfirmedBlocksWithLimit)](jsonrpc-api.md#getconfirmedblockswithlimit)
- [الحصول على التوقيعات المُؤَكدة للعنوان 2 (getConfirmedSignaturesForAddress2)](jsonrpc-api.md#getconfirmedsignaturesforaddress2)
- [الحصول على المُعاملة المُؤَكدة (getConfirmedTransaction)](jsonrpc-api.md#getconfirmedtransaction)

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

- `"finalized"` - the node will query the most recent block confirmed by supermajority of the cluster as having reached maximum lockout, meaning the cluster has recognized this block as finalized
- `"confirmed"` - the node will query the most recent block that has been voted on by supermajority of the cluster.
  - يتضمن أصوات القيل والقال (gossip) وإعادتها.
  - فهو لا يحسب الأصوات على سُلالة مُتحدِّري (descendants) الكتلة (block)، بل يُوجهون الأصوات على تلك الكتلة (block).
  - يدعم مُستوى التأكيد هذا أيضا ضمانات "التأكيد المُتفائل" (optimistic confirmation) في الإصدار 1.3 وما بعده.
- `"processed"` - the node will query its most recent block. لاحظ أن الكتلة (block) قد لا تكون كاملة.

For processing many dependent transactions in series, it's recommended to use `"confirmed"` commitment, which balances speed with rollback safety. For total safety, it's recommended to use`"finalized"` commitment.

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
        "commitment": "finalized"
      }
    ]
  }
'
```

#### المُفتَرَض (Default):

If commitment configuration is not provided, the node will default to `"finalized"` commitment

فقط الطرق التي تستفسر عن حالة البنك تقبل مُعلِّمة (parameter) الإلتزام. مُشار إليها في مرجع واجهة برمجة التطبيقات (API) الوارد أدناه.

#### هيكل إستجابة الـ Rpc أو RpcResponse Structure

العديد من الطرق التي تأخذ مُعلِّمة (parameter) إلتزام تُعيد كائن RpcResponse JSON مُكون من جزأين:

- السياق `context`: بنية RpcResponseContext JSON بما في ذلك حقل فُتحة `slot` حيث تم تقييم العملية.
- القيمة `value`: القيمة التي تم إرجاعها من قبل العملية نفسها.

## فحص الصِّحَّة (Health Check)

على الرغم من أنه ليس JSON RPC API، يُوفر `GET /Health` في نقطة نهاية RPC HTTP آلية فحص صِّحّي لإستخدامها من قبل مُوازن التحميل (load balancers) أو أي شبكة بنية تحتية أخرى. This request will always return a HTTP 200 OK response with a body of "ok", "behind" or "unknown" based on the following conditions:

1. If one or more `--trusted-validator` arguments are provided to `solana-validator`, "ok" is returned when the node has within `HEALTH_CHECK_SLOT_DISTANCE` slots of the highest trusted validator, otherwise "behind". "unknown" is returned when no slot information from trusted validators is not yet available.
2. رسالة "ok" تُعاد دائماً إذا لم يتم توفير أي مُدقّقين (validators) موثوقين.

## مرجع JSON RPC API

### الحصول على معلومات الحساب (getAccountInfo)

يُرجع جميع المعلومات المُرتبطة بالحساب المُقدم للمفتاح العمومي (pubkey)

#### المُعلمات (parameters):

- `<string>` - المفتاح العمومي (pubkey) للحساب الذي سيتم الإستفسار عنه، كسلسلة مُرمّزة base-58
- `<object>` - كائن تكوين (إختياري) يحتوي على الحقول الإختيارية التالية:
  - (إختياري) الإلتزام [Commitment](jsonrpc-api.md#configuring-state-commitment)
  - الترميز `encoding: <string>` - الترميز لبيانات الحساب، إما "base58" بطيء "(_slow_) ، "base64", "base64+zstd" ، أو "jsonParsed". يقتصر "base58" على بيانات الحساب التي تقل عن 129 bytes. سيقوم "base64" بإرجاع البيانات المُرمّزة لـ base64 لبيانات الحساب من أي حجم. "base64+zstd" يضغط على بيانات الحساب بإستخدام [Zstandard](https://facebook.github.io/zstd/) و base64-en النتيجة. يُحاول ترميز "jsonParsed" إستخدام مُوزعي الحالة الخاصين بالبرنامج لإرجاع المزيد من بيانات حالة الحساب التي يُمكن قراءتها بشكل واضح. إذا طُلب "jsonParsed" ولكن لا يُمكن العثور على مُحلل (parser)، يعود الحقل إلى الترميز "base64"، يُمكن الكشف عنها عندما تكون بيانات `data` الحقل هو النوع `<string>`.
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

### getBlock

يُرجع معلومات الهوية والمُعاملات حول كتلة (block) مُؤكدة في دفتر الأستاذ (ledger)

#### المُعلمات (parameters):

- `<u64>` - الفُتحة (slot)، كرقم صحيح u64
- `<object>` - (إختياري) تحتوي إعدادات الكائن على الحقول الإختيارية التالية:
  - (optional) `encoding: <string>` - encoding for each returned Transaction, either "json", "jsonParsed", "base58" (_slow_), "base64". إذا لم يتم توفير المُعلِّمة (parameter)، فإن الترميز المُفترض (Default) هو "json". يُحاول ترميز "jsonParsed" إستخدام مُحلِّلي (parsers) التعليمات الخاصة بالبرامج لإرجاع بيانات أكثر قابلية للقراءة ووضوحا في قائمة تعليمات.معاملة. الرسالة `transaction.message.instructions`. إذا كانت "jsonParsed" مطلوبة ولكن لا يمكن العثور على مُحلل (parser)، فإن التعليمات تعود إلى ترميز JSON العادي للحسابات `accounts` ، البيانات `data` وحقول مُعرف البرنامج `programIdIndex`.
  - (optional) `transactionDetails: <string>` - level of transaction detail to return, either "full", "signatures", or "none". If parameter not provided, the default detail level is "full".
  - (optional) `rewards: bool` - whether to populate the `rewards` array. If parameter not provided, the default includes rewards.
  - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment); "processed" is not supported. If parameter not provided, the default is "finalized".

#### النتائج:

سيكون ناتج الإستجابة (response output) كائن Json مع الحقول التالية:

- `<null>` - إذا لم يتم تأكيد الكتلة (block) المُحَدَّدة
- `<object>` - إذا تم تأكيد الكتلة (block)، كائن مع الحقول التالية:
  - تجزئة الكتلة `blockhash: <string>` - تجزئة الكتلة (blockhash) لهذه الكتلة، كسلسلة مُرمّزة base-58
  - تجزئة الكتلة السابقة `previousBlockhash: <string>` - تجزئة الكتلة (blockhash) من أصل هذه الكتلة (block)، كسلسلة مُرمّزة base-58؛ إذا كانت الكتلة (block) الأصلية غير مُتوفرة بسبب تنظيف دفتر الأستاذ (ledger)، فسيعود هذا الحقل إلى "111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111 "
  - الفُتحة الأصل `parentSlot: <u64>` - فهرس الفُتحة (slot) لهذه الكتلة الأصل
  - `transactions: <array>` - present if "full" transaction details are requested; an array of JSON objects containing:
    - المُعاملة `transaction: <object|[string,encoding]>` - [Transaction](#transaction-structure) كائن، إما بصيغة JSON أو البيانات الثنائية المُرمّزة (encoded binary data)، إعتماداً على مُعلِّمة (parameter) الترميز
    - `meta: <object>` - كائن بيانات تعريف حالة المُعاملة، يحتوي على لاغِ `null` أو:
      - خطأ `err: <object | null>` - خطأ إذا فشلت المُعاملة، لاغِ إذا نجحت المُعاملة. [TransactionError definitions](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
      - رسوم `fee: <u64>` - رسوم هذه المُعاملة، كما هو عدد صحيح في u64
      - ما قبل الرصيد `preBalances: <array>` - مجموعة من أرصدة حساب u64 قبل مُعالجة المُعاملة
      - ما بعد الرصيد `postBalances: <array>` - مجموعة من أرصدة حساب u64 بعد معالجة المعاملة
      - التعليمات الداخلية `innerInstructions: <array|undefined>` - قائمة التعليمات [inner instructions](#inner-instructions-structure) الداخلية أو محذوفة إذا لم يتم تمكين تسجيل التعليمات الداخلية أثناء هذه المُعاملة
      - `preTokenBalances: <array|undefined>` - List of [token balances](#token-balances-structure) from before the transaction was processed or omitted if token balance recording was not yet enabled during this transaction
      - `postTokenBalances: <array|undefined>` - List of [token balances](#token-balances-structure) from after the transaction was processed or omitted if token balance recording was not yet enabled during this transaction
      - سجلات الرسائل `logMessages: <array>` - مجموعة من رسائل سجل السلسلة أو تم حذفها إذا لم يتم تمكين تسجيل رسائل السجل أثناء هذه المُعاملة
      - إهمال: حالة `status: <object>` - حالة المُعاملة
        - `"Ok": <null>` - العملية كانت ناجحة
        - خطأ `"err": <ERR>` - فشلت المُعاملة مع خطأ (TransactionError) في المُعاملة
  - `signatures: <array>` - present if "signatures" are requested for transaction details; an array of signatures strings, corresponding to the transaction order in the block
  - `rewards: <array>` - present if rewards are requested; an array of JSON objects containing:
    - المفتاح العمومي `pubkey: <string>` - المفتاح العمومي (public key)، كسلسلة مُرمّزة base-58، للحساب الذي تلقى المُكافأة
    - `الlamports: <i64>`عدد الـ lamports المُخَصَّصَة لهذا الحساب، كـ u64
    - ما بعد الرصيد `postBalance: <u64>` - رصيد الحساب في الـ lamports بعد تطبيق المُكافأة
    - نوع المُكافأة `rewardType: <string|undefined>` - نوع المُكافأة: "الرسوم" (fee)، "الإيجار" (rent)، "التصويت" (voting)، "التَّحْصِيص" (staking)
  - وقت الكتلة `blockTime: <i64 | null>` - الوقت التقديري للإنتاج، كتوقيت الختم الزمني Unix (ثواني منذ الفترة Unix). لاغية إذا لم تكن مُتوفرة

#### مثال:

الطلب:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc": "2.0","id":1,"method":"getBlock","params":[430, {"encoding": "json","transactionDetails":"full","rewards":false}]}
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
    "transactions": [
      {
        "meta": {
          "err": null,
          "fee": 5000,
          "innerInstructions": [],
          "logMessages": [],
          "postBalances": [499998932500, 26858640, 1, 1, 1],
          "postTokenBalances": [],
          "preBalances": [499998937500, 26858640, 1, 1, 1],
          "preTokenBalances": [],
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
  {"jsonrpc": "2.0","id":1,"method":"getBlock","params":[430, "base64"]}
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
          "postTokenBalances": [],
          "preBalances": [499998937500, 26858640, 1, 1, 1],
          "preTokenBalances": [],
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
  - فهرس مُعرف البرنامج `programIdIndex: <number>` - فهرس في مفاتيح حساب الرسالة `message.accountKeys` مع الإشارة إلى حساب البرنامج الذي يُنفذ هذه التعليمة.
  - الحسابات `accounts: <array[number]>` - قائمة المؤشرات المُرتبة في مفاتيح حساب الرسالة `message.accountKeys` تُبين أي الحسابات التي ستمر إلى البرنامج.
  - البيانات `data: <string>` - بيانات إدخال البرنامج مُرمّزة في سلسلة base-58.

#### Token Balances Structure

The JSON structure of token balances is defined as a list of objects in the following structure:

- `accountIndex: <number>` - Index of the account in which the token balance is provided for.
- `mint: <string>` - Pubkey of the token's mint.
- `uiTokenAmount: <object>` -
  - `amount: <string>` - Raw amount of tokens as a string, ignoring decimals.
  - `decimals: <number>` - Number of decimals configured for token's mint.
  - `uiAmount: <number | null>` - Token amount as a float, accounting for decimals. **DEPRECATED**
  - `uiAmountString: <string>` - Token amount as a string, accounting for decimals.

### getBlockProduction

Returns recent block production information from the current or previous epoch.

#### المُعلمات (parameters):

- `<object>` - (إختياري) تحتوي إعدادات الكائن على الحقول الإختيارية التالية:
  - (إختياري) الإلتزام [Commitment](jsonrpc-api.md#configuring-state-commitment)
  - (optional) `range: <object>` - Slot range to return block production for. إذا لم يتم توفير المُعلِّمة (parameter)، فإن الإفتراضات (Defaults) للفترة (epoch) الحالية.
    - `firstSlot: <u64>` - first slot to return block production information for (inclusive)
    - (optional) `lastSlot: <u64>` - last slot to return block production information for (inclusive). If parameter not provided, defaults to the highest slot
  - (optional) `identity: <string>` - Only return results for this validator identity (base-58 encoded)

#### النتائج:

ستكون النتيجة كائن RpcResponse JSON بقيمة `value` تُساوي:

- `<object>`
  - `byIdentity: <object>` - a dictionary of validator identities, as base-58 encoded strings. Value is a two element array containing the number of leader slots and the number of blocks produced.
  - `range: <object>` - Block production slot range
    - `firstSlot: <u64>` - first slot of the block production information (inclusive)
    - `lastSlot: <u64>` - last slot of block production information (inclusive)

#### مثال:

الطلب:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getBlockProduction"}
'
```

النتيجة:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 9887
    },
    "value": {
      "byIdentity": {
        "85iYT5RuzRTDgjyRa3cP8SYhM2j21fj7NhfJ3peu1DPr": [9888, 9886]
      },
      "range": {
        "firstSlot": 0,
        "lastSlot": 9887
      }
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
    "method": "getBlockProduction",
    "params": [
      {
        "identity": "85iYT5RuzRTDgjyRa3cP8SYhM2j21fj7NhfJ3peu1DPr",
        "range": {
          "firstSlot": 40,
          "lastSlot": 50
        }
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
      "slot": 10102
    },
    "value": {
      "byIdentity": {
        "85iYT5RuzRTDgjyRa3cP8SYhM2j21fj7NhfJ3peu1DPr": [11, 11]
      },
      "range": {
        "firstSlot": 50,
        "lastSlot": 40
      }
    }
  },
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

### getBlocks

يرجع قائمة بالكتل (blocks) المُؤَكدة بين فُتحتين (slots)

#### المُعلمات (parameters):

- `<u64>` - تشغيل الفُتحة (start_slot)، كعدد صحيح u64
- `<u64>` - (إختياري) تشغيل الفُتحة، كعدد صحيح u64
- (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment); "processed" is not supported. If parameter not provided, the default is "finalized".

#### النتائج:

سيكون حقل النتيجة مجموعة من عدد صحيح من u64 يُورد كتل (blocks) مُؤَكدة بين `start_slot` وإما `end_slo`، إذا قدمت، أو أحدث كتلة مُؤَكدة ، شاملة. الحد الأقصى المسموح به هو 500,000 فُتحة (slots).

#### مثال:

الطلب:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc": "2.0","id":1,"method":"getBlocks","params":[5, 10]}
'
```

النتيجة:

```json
{ "jsonrpc": "2.0", "result": 1574721591, "id": 1 }
```

### getBlocksWithLimit

يُرجع قائمة بالكتل (blocks) المُؤَكدة بدءاً من فُتحة (slot) مُعينة

#### المُعلمات (parameters):

- `<u64>` - تشغيل الفُتحة (start_slot)، كعدد صحيح u64
- `<u64>` - حد الفُتحة (slot) كعدد صحيح u64
- (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment); "processed" is not supported. If parameter not provided, the default is "finalized".

#### النتائج:

سيكون حقل النتيجة مجموعة من عدد صحيح من u64 يورد كتل (blocks) مؤكدة بدءاً من `start_slo` حتى `limit` الكتل (blocks)، الشامل.

#### مثال:

الطلب:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc": "2.0","id":1,"method":"getBlocksWithLimit","params":[5, 3]}
'
```

النتيجة:

```json
{ "jsonrpc": "2.0", "result": [5, 6, 7], "id": 1 }
```

### الحصول على وقت الكتلة (getBlockTime)

Returns the estimated production time of a block.

يقوم كل مُدقّق (validator) بالإبلاغ عن وقت UTC الخاص به إلى دفتر الأستاذ (ledger) بفترة زمنية مُنتظمة عن طريق إضافة ختم زمني (Timestamp) بشكل مُتقطع إلى تصويت كتلة (block) مُعينة. يتم حساب وقت الكتلة (block time) المطلوبة من المُرَجَّح بالحِصَّة (stake-weighted) لتصويت الأختام الزمنية (Timestamps) في مجموعة من الكتل (blocks) الحديثة المُسجلة في دفتر الأستاذ (ledger).

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

### الحصول على معلومات الفترة (getEpochInfo)

إرجاع معلومات حول الفترة (epoc) الحالية

#### المُعلمات (parameters):

- `<object>` (إختياري) الإلتزام [Commitment](jsonrpc-api.md#configuring-state-commitment)

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
- `<object>` (إختياري) الإلتزام [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### النتائج:

ستكون النتيجة كائن RpcResponse JSON بقيمة `value` تُساوي:

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

- `<object>` (إختياري) الإلتزام [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### النتائج:

ستكون النتيجة كائن JSON RpcResponse مع قيمة `value` مجموعة إلى كائن JSON مع الحقول التالية:

- تجزئة الكُتلة `blockhash: <string>` - تجزئة الكُتلة (blockhash) كسلسلة مُرمّزة base-58
- `feeCalculator: <object>` - عنصر حاسبة الرسوم (FeeCalculator)، جدول رسوم لتجزئة الكُتلة (blockhash) هذه
- `lastValidSlot: <u64>` - DEPRECATED - this value is inaccurate and should not be relied upon

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

- `<object>` (إختياري) الإلتزام [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### النتائج:

سيكون ناتج الإستجابة (response output) كائن Json مع الحقول التالية:

- `initial: <f64>`، نسبة التَضَخُّم الأولية من الوقت 0
- `terminal: <f64>`، نسبة التَضَخُّم النهائي
- `taper: <f64>`، المُعدل السنوي الذي يتم فيه خفض التَضَخُّم. Rate reduction is derived using the target slot time in genesis config
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

### getInflationReward

Returns the inflation reward for a list of addresses for an epoch

#### المُعلمات (parameters):

- `<array>` - An array of addresses to query, as base-58 encoded strings

* `<object>` - (إختياري) تحتوي إعدادات الكائن على الحقول الإختيارية التالية:
  - (إختياري) الإلتزام [Commitment](jsonrpc-api.md#configuring-state-commitment)
  - (optional) `epoch: <u64>` - An epoch for which the reward occurs. If omitted, the previous epoch will be used

#### النتائج

The result field will be a JSON array with the following fields:

- `epoch: <u64>`, epoch for which reward occured
- `effectiveSlot: <u64>`, the slot in which the rewards are effective
- `amount: <u64>`, reward amount in lamports
- `postBalance: <u64>`, post balance of the account in lamports

#### مثال (Example)

الطلب:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getInflationReward",
    "params": [
       ["6dmNQ5jwLeLk5REvio1JcMshcbvkYMwy26sJ8pbkvStu", "BGsqMegLpV6n6Ve146sSX2dTjUMj3M92HnU8BbNRMhF2"], 2
    ]
  }
'
```

الإستجابة:

```json
{
  "jsonrpc": "2.0",
  "result": [
    {
      "amount": 2500,
      "effectiveSlot": 224,
      "epoch": 2,
      "postBalance": 499999442500
    },
    null
  ],
  "id": 1
}
```

### الحصول على أكبر الحِسابات (getLargestAccounts)

Returns the 20 largest accounts, by lamport balance (results may be cached up to two hours)

#### المُعلمات (parameters):

- `<object>` - (إختياري) تحتوي إعدادات الكائن على الحقول الإختيارية التالية:
  - (إختياري) الإلتزام [Commitment](jsonrpc-api.md#configuring-state-commitment)
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
- `<object>` - كائن تكوين (إختياري) يحتوي على الحقول الإختيارية التالية:
  - (إختياري) الإلتزام [Commitment](jsonrpc-api.md#configuring-state-commitment)
  - (optional) `identity: <string>` - Only return results for this validator identity (base-58 encoded)

#### النتائج:

- `<null>` - إذا لم يتم إيجاد الفترة (epoch) المطلوبة
- `<object>` - otherwise, the result field will be a dictionary of validator identities, as base-58 encoded strings, and their corresponding leader slot indices as values (indices are relative to the first slot in the requested epoch)

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

#### مثال:

الطلب:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getLeaderSchedule",
    "params": [
      null,
      {
        "identity": "4Qkev8aNZcqFNSRhQzwyLMFSsi94jHqE8WNVTJzTP99F"
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

### getMaxRetransmitSlot

Get the max slot seen from retransmit stage.

#### النتائج:

- `<u64>` - الفُتحة (Slot)

#### مثال:

الطلب:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getMaxRetransmitSlot"}
'
```

النتيجة:

```json
{ "jsonrpc": "2.0", "result": 1234, "id": 1 }
```

### getMaxShredInsertSlot

Get the max slot seen from after shred insert.

#### النتائج:

- `<u64>` - الفُتحة (Slot)

#### مثال:

الطلب:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getMaxShredInsertSlot"}
'
```

النتيجة:

```json
{ "jsonrpc": "2.0", "result": 1234, "id": 1 }
```

### الحصول على الحد الأدنى للرصيد للإعفاء من الإيجار (getMinimumBalanceForRentExemption)

يُرجع الحد الأدنى من الرصيد المطلوب لإعفاء الحساب من الإيجار.

#### المُعلمات (parameters):

- `<usize>` - طول بيانات الحساب
- `<object>` (إختياري) الإلتزام [Commitment](jsonrpc-api.md#configuring-state-commitment)

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

### الحصول على حسابات مُتعددة (getMultipleAccounts)

يُرجع معلومات الحِساب لقائمة من المفاتيح العمومية (Pubkeys)

#### المُعلمات (parameters):

- `<array>` - المفتاح العمومي (pubkey) للحِساب الذي سيتم الإستفسار عنه، كسلسلة مُرمّزة base-58
- `<object>` - (إختياري) تحتوي إعدادات الكائن على الحقول الإختيارية التالية:
  - (إختياري) الإلتزام [Commitment](jsonrpc-api.md#configuring-state-commitment)
  - الترميز `encoding: <string>` - الترميز لبيانات الحساب، إما "base58" بطيء "(_slow_) ، "base64", "base64+zstd" ، أو "jsonParsed". "base58" is limited to Account data of less than 129 bytes. سيقوم "base64" بإرجاع البيانات المُرمّزة لـ base64 لبيانات الحساب من أي حجم. "base64+zstd" يضغط على بيانات الحساب بإستخدام [Zstandard](https://facebook.github.io/zstd/) و base64-en النتيجة. يُحاول ترميز "jsonParsed" إستخدام مُوزعي الحالة الخاصين بالبرنامج لإرجاع المزيد من بيانات حالة الحساب التي يُمكن قراءتها بشكل واضح. إذا طُلب "jsonParsed" ولكن لا يُمكن العثور على مُحلل (parser)، يعود الحقل إلى الترميز "base64"، يُمكن الكشف عنها عندما تكون بيانات `data` الحقل هو النوع `<string>`.
  - (إختياري) شريحة البيانات `dataSlice: <object>` - الحد من بيانات الحساب التي تم إرجاعها بإستخدام المُوازنة المُقدمة `: <usize>` و طول `length: <usize>` الحقول؛ مُتاح فقط لترميز "base58", "base64" أو "base64+zstd".

#### النتائج:

ستكون النتيجة كائن RpcResponse JSON بقيمة `value` تُساوي:

مجموعة من:

- `<null>` - إذا كان الحساب المطلوب لذلك المفتاح العمومي (Pubkey) غير موجود
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

### الحصول على حسابات البرنامج (getProgramAccounts)

يُرجع جميع الحسابات المملوكة للبرنامج المزود للمفتاح العمومي (pubkey)

#### المُعلمات (parameters):

- `<string>` - المفتاح العمومي (pubkey) للبرنامج الذي سيتم الإستفسار عنه، كسلسلة مُرمّزة base-58
- `<object>` - (إختياري) تحتوي إعدادات الكائن على الحقول الإختيارية التالية:
  - (إختياري) الإلتزام [Commitment](jsonrpc-api.md#configuring-state-commitment)
  - الترميز `encoding: <string>` - الترميز لبيانات الحساب، إما "base58" بطيء "(_slow_) ، "base64", "base64+zstd" ، أو "jsonParsed". "base58" is limited to Account data of less than 129 bytes. سيقوم "base64" بإرجاع البيانات المُرمّزة لـ base64 لبيانات الحساب من أي حجم. "base64+zstd" يضغط على بيانات الحساب بإستخدام [Zstandard](https://facebook.github.io/zstd/) و base64-en النتيجة. يُحاول ترميز "jsonParsed" إستخدام مُوزعي الحالة الخاصين بالبرنامج لإرجاع المزيد من بيانات حالة الحساب التي يُمكن قراءتها بشكل واضح. إذا طُلب "jsonParsed" ولكن لا يُمكن العثور على مُحلل (parser)، يعود الحقل إلى الترميز "base64"، يُمكن الكشف عنها عندما تكون بيانات `data` الحقل هو النوع `<string>`.
  - (إختياري) شريحة البيانات `dataSlice: <object>` - الحد من بيانات الحساب التي تم إرجاعها بإستخدام المُوازنة المُقدمة `: <usize>` و طول `length: <usize>` الحقول؛ مُتاح فقط لترميز "base58", "base64" أو "base64+zstd".
  - (اختياري) `filters: <array>` - نتائج الفلترة بإستخدام كائنات فلترة مُختلفة [filter objects](jsonrpc-api.md#filters); الحساب يجب أن يفي بجميع معايير الفلترة لتضمينها في النتائج

##### الفلاتر (Filters):

- `memcmp: <object>` - مُقارنة سلسلة من الـ bytes التي تم توفيرها مع بيانات حِساب البرنامج في إزاحة (offset) مُعينة. الحقول (Fields):

  - `offset: <usize>` - الإزاحة في بيانات حساب البرنامج لبدء المُقارنة
  - `bytes: <string>` - data to match, as base-58 encoded string and limited to less than 129 bytes

- حجم البيانات `dataSize: <u64>` - مقارنة طول بيانات حساب البرنامج مع حجم البيانات المُقدمة

#### النتائج:

سيكون حقل النتيجة عبارة عن مجموعة من الكائنات JSON، التي ستحتوي على:

- `pubkey: <string>` - المفتاح العمومي (public key) كسلسلة مُرمّزة base-58
- `account: <object>` - كائن JSON، مع الحقول الفرعية التالية:
  - الـ `lamports: <u64>`، عدد الـ lamports المُخَصَّصَة لهذا الحساب، كـ u64
  - المالك `owner: <string>`، المفتاح العمومي المُرمّز base-58 للبرنامج الذي تم تعيين هذا الحساب له `data: <[string,encoding]|object>`، البيانات المُرتبطة بالحِساب، إما كبيانات ثنائية مُشَفَّرَة أو شكل `{<program>: <state>}`، إعتمادًا على مُعلِّمة (parameter) الترميز
  - `executable: <bool>`، المنطقية (boolean) تُشير إلى ما إذا كان الحِساب يحتوي على برنامج \(وهو للقراءة-فقط\)
  - `إيجار الفترة: <u64>`، الفترة (epoch) التي سيكون فيها هذا الحساب مدينا بالإيجار القادم، مثل u64

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

- `<object>` (إختياري) الإلتزام [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### النتائج:

Rpcresponse يحتوي على كائن JSON يتكون من كائن JSON لسلسلة تجزئة الكُتلة (Blockhash) وكائن حاسبة رسوم JSON أو FeeCalculator JSON.

- `RpcResponse<object>` - كائن هيكل إستجابة JSON مع قِيمة `value` تعيين الحقل إلى كائن JSON بما في ذلك:
- تجزئة الكُتلة `blockhash: <string>` - تجزئة الكُتلة (blockhash) كسلسلة مُرمّزة base-58
- `feeCalculator: <object>` - عنصر حاسبة الرسوم (FeeCalculator)، جدول رسوم لتجزئة الكُتلة (blockhash) هذه

#### مثال:

الطلب:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getRecentBlockhash"}
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

### getSignaturesForAddress

إرجاع التوقيعات المُؤَكدة للمعاملات التي تتضمن ملف العنوان إلى الوراء في الوقت المُناسب من التوقيع المُقدم أو أحدث كتلة (block) مُؤَكدة

#### المُعلمات (parameters):

- `<string>` - المفتاح العمومي (pubkey) للحساب الذي سيتم الإستفسار عنه، كسلسلة مُرمّزة base-58
- `<object>` - كائن تكوين (إختياري) يحتوي على الحقول الإختيارية التالية:
  - الحد الأقصى `limit: <number>` - (إختياري) الحد الأقصى لتوقيعات المُعاملة للإرجاع (بين 1 و 1000، الإفتراض: 1000).
  - قبل `before: <string>` - (إختياري) بدء البحث إلى الوراء من توقيع هذه المُعاملة. في حالة عدم تقديمه، يبدأ البحث من أقصى أعلى كتلة (block) مُؤَكدة.
  - حتى`until: <string>` - (إختياري) البحث حتى توقيع هذه المُعاملة، إذا وجدت قبل الوصول إلى الحد الأقصى.
  - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment); "processed" is not supported. If parameter not provided, the default is "finalized".

#### النتائج:

سيكون حقل النتيجة عبارة عن مجموعة من معلومات توقيع المُعاملة، مُرتبة من المُعاملة الأحدث إلى الأقدم:

- `<object>`
  - `signature: <string>` - توقيع المُعاملة كسلسلة مُرمّزة base-58
  - الفُتحة `slot: <u64>` - الفُتحة (slot) التي تحتوي على الكتلة (block) مع المُعاملة
  - خطأ `err: <object | null>` - خطأ إذا فشلت المُعاملة، لاغِ إذا نجحت المُعاملة. [TransactionError definitions](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
  - المُذكرة `memo: <string |null>` - مُذكرة مُرتبطة بالمُعاملة، لاغية إذا لم تكن هناك مُذكرة موجودة
  - `blockTime: <i64 | null>` - estimated production time, as Unix timestamp (seconds since the Unix epoch) of when transaction was processed. null if not available.

#### مثال:

الطلب:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getSignaturesForAddress",
    "params": [
      "Vote111111111111111111111111111111111111111",
      {
        "limit": 1
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
      "err": null,
      "memo": null,
      "signature": "5h6xBEauJ3PK6SWCZ1PGjBvj8vDdWG3KpwATGy1ARAXFSDwt8GFXM7W5Ncn16wmqokgpiKRLuS83KUxyZyv2sUYv",
      "slot": 114,
      "blockTime": null
    }
  ],
  "id": 1
}
```

### الحصول على حالات التوقيع (getSignatureStatuses)

يُرجع حالات لقائمة التوقيعات. Unless the `searchTransactionHistory` configuration parameter is included, this method only searches the recent status cache of signatures, which retains statuses for all active slots plus `MAX_RECENT_BLOCKHASHES` rooted slots.

#### Parameters:

- `<array>` - An array of transaction signatures to confirm, as base-58 encoded strings
- `<object>` - (optional) Configuration object containing the following field:
  - `searchTransactionHistory: <bool>` - if true, a Solana node will search its ledger cache for any signatures not found in the recent status cache

#### Results:

An RpcResponse containing a JSON object consisting of an array of TransactionStatus objects.

- `RpcResponse<object>` - RpcResponse JSON object with `value` field:

An array of:

- `<null>` - Unknown transaction
- `<object>`
  - `slot: <u64>` - The slot the transaction was processed
  - `confirmations: <usize | null>` - Number of blocks since signature confirmation, null if rooted, as well as finalized by a supermajority of the cluster
  - `err: <object | null>` - Error if transaction failed, null if transaction succeeded. [TransactionError definitions](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
  - `confirmationStatus: <string | null>` - The transaction's cluster confirmation status; either `processed`, `confirmed`, or `finalized`. See [Commitment](jsonrpc-api.md#configuring-state-commitment) for more on optimistic confirmation.
  - DEPRECATED: `status: <object>` - Transaction status
    - `"Ok": <null>` - Transaction was successful
    - `"Err": <ERR>` - Transaction failed with TransactionError

#### Example:

Request:

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

Result:

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
    "method": "getSignatureStatuses",
    "params": [
      [
        "5VERv8NMvzbJMEkV8xnrLkEaWRtSz9CosKDYjCJjBRnbJLgp8uirBgmQpjKhoR4tjF3ZpRzrFmBV6UjKdiSZkQUW"
      ],
      {
        "searchTransactionHistory": true
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

### getSlot

Returns the current slot the node is processing

#### المُعلمات (parameters):

- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### النتائج:

- `<u64>` - Current slot

#### مثال:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getSlot"}
'
```

Result:

```json
{ "jsonrpc": "2.0", "result": 1234, "id": 1 }
```

### getSlotLeader

Returns the current slot leader

#### Parameters:

- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

- `<string>` - Node identity Pubkey as base-58 encoded string

#### Example:

الطلب:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getSlotLeader"}
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

### getSlotLeaders

Returns the slot leaders for a given slot range

#### Parameters:

- `<u64>` - Start slot, as u64 integer
- `<u64>` - Limit, as u64 integer

#### Results:

- `<array<string>>` - Node identity public keys as base-58 encoded strings

#### Example:

If the current slot is #99, query the next 10 leaders with the following request:

الطلب:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getSlotLeaders", "params":[100, 10]}
'
```

النتيجة:

The first leader returned is the leader for slot #100:

```json
{
  "jsonrpc": "2.0",
  "result": [
    "ChorusmmK7i1AxXeiTtQgQZhQNiXYU84ULeaYF1EH15n",
    "ChorusmmK7i1AxXeiTtQgQZhQNiXYU84ULeaYF1EH15n",
    "ChorusmmK7i1AxXeiTtQgQZhQNiXYU84ULeaYF1EH15n",
    "ChorusmmK7i1AxXeiTtQgQZhQNiXYU84ULeaYF1EH15n",
    "Awes4Tr6TX8JDzEhCZY2QVNimT6iD1zWHzf1vNyGvpLM",
    "Awes4Tr6TX8JDzEhCZY2QVNimT6iD1zWHzf1vNyGvpLM",
    "Awes4Tr6TX8JDzEhCZY2QVNimT6iD1zWHzf1vNyGvpLM",
    "Awes4Tr6TX8JDzEhCZY2QVNimT6iD1zWHzf1vNyGvpLM",
    "DWvDTSh3qfn88UoQTEKRV2JnLt5jtJAVoiCo3ivtMwXP",
    "DWvDTSh3qfn88UoQTEKRV2JnLt5jtJAVoiCo3ivtMwXP"
  ],
  "id": 1
}
```

### getStakeActivation

Returns epoch activation information for a stake account

#### Parameters:

- `<string>` - Pubkey of stake account to query, as base-58 encoded string
- `<object>` - (optional) Configuration object containing the following optional fields:
  - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)
  - (optional) `epoch: <u64>` - epoch for which to calculate activation details. If parameter not provided, defaults to current epoch.

#### Results:

The result will be a JSON object with the following fields:

- `state: <string` - the stake account's activation state, one of: `active`, `inactive`, `activating`, `deactivating`
- `active: <u64>` - stake active during the epoch
- `inactive: <u64>` - stake inactive during the epoch

#### Example:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getStakeActivation", "params": ["CYRJWqiSjLitBAcRxPvWpgX3s5TvmN2SuRY3eEYypFvT"]}
'
```

Result:

```json
{
  "jsonrpc": "2.0",
  "result": { "active": 197717120, "inactive": 0, "state": "active" },
  "id": 1
}
```

#### مثال:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getStakeActivation",
    "params": [
      "CYRJWqiSjLitBAcRxPvWpgX3s5TvmN2SuRY3eEYypFvT",
      {
        "epoch": 4
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

### getSupply

Returns information about the current supply.

#### المُعلمات (parameters):

- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### النتائج:

The result will be an RpcResponse JSON object with `value` equal to a JSON object containing:

- `total: <u64>` - Total supply in lamports
- `circulating: <u64>` - Circulating supply in lamports
- `nonCirculating: <u64>` - Non-circulating supply in lamports
- `nonCirculatingAccounts: <array>` - an array of account addresses of non-circulating accounts, as strings

#### مثال:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getSupply"}
'
```

Result:

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

### getTokenAccountBalance

Returns the token balance of an SPL Token account.

#### المُعلمات (parameters):

- `<string>` - Pubkey of Token account to query, as base-58 encoded string
- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### النتائج:

The result will be an RpcResponse JSON object with `value` equal to a JSON object containing:

- `amount: <string>` - the raw balance without decimals, a string representation of u64
- `decimals: <u8>` - number of base 10 digits to the right of the decimal place
- `uiAmount: <number | null>` - the balance, using mint-prescribed decimals **DEPRECATED**
- `uiAmountString: <string>` - the balance as a string, using mint-prescribed decimals

#### مثال:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getTokenAccountBalance", "params": ["7fUAJdStEuGbc3sM84cKRL6yYaaSstyLSU4ve5oovLS7"]}
'
```

Result:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1114
    },
    "value": {
      "amount": "9864",
      "decimals": 2,
      "uiAmount": 98.64,
      "uiAmountString": "98.64"
    },
    "id": 1
  }
}
```

### getTokenAccountsByDelegate

Returns all SPL Token accounts by approved Delegate.

#### المُعلمات (parameters):

- `<string>` - Pubkey of account delegate to query, as base-58 encoded string
- `<object>` - Either:
  - `mint: <string>` - Pubkey of the specific token Mint to limit accounts to, as base-58 encoded string; or
  - `programId: <string>` - Pubkey of the Token program ID that owns the accounts, as base-58 encoded string
- `<object>` - (optional) Configuration object containing the following optional fields:
  - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)
  - `encoding: <string>` - encoding for Account data, either "base58" (_slow_), "base64", "base64+zstd" or "jsonParsed". "jsonParsed" encoding attempts to use program-specific state parsers to return more human-readable and explicit account state data. If "jsonParsed" is requested but a valid mint cannot be found for a particular account, that account will be filtered out from results.
  - (optional) `dataSlice: <object>` - limit the returned account data using the provided `offset: <usize>` and `length: <usize>` fields; only available for "base58", "base64" or "base64+zstd" encodings.

#### النتائج:

The result will be an RpcResponse JSON object with `value` equal to an array of JSON objects, which will contain:

- `pubkey: <string>` - the account Pubkey as base-58 encoded string
- `account: <object>` - a JSON object, with the following sub fields:
  - `lamports: <u64>`, number of lamports assigned to this account, as a u64
  - `owner: <string>`, base-58 encoded Pubkey of the program this account has been assigned to
  - `data: <object>`, Token state data associated with the account, either as encoded binary data or in JSON format `{<program>: <state>}`
  - `executable: <bool>`, boolean indicating if the account contains a program \(and is strictly read-only\)
  - `rentEpoch: <u64>`, the epoch at which this account will next owe rent, as u64

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
                "decimals": 1,
                "uiAmount": 0.1,
                "uiAmountString": "0.1"
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

### getTokenAccountsByOwner

Returns all SPL Token accounts by token owner.

#### المُعلمات (parameters):

- `<string>` - Pubkey of account owner to query, as base-58 encoded string
- `<object>` - Either:
  - `mint: <string>` - Pubkey of the specific token Mint to limit accounts to, as base-58 encoded string; or
  - `programId: <string>` - Pubkey of the Token program ID that owns the accounts, as base-58 encoded string
- `<object>` - (optional) Configuration object containing the following optional fields:
  - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)
  - `encoding: <string>` - encoding for Account data, either "base58" (_slow_), "base64", "base64+zstd" or "jsonParsed". "jsonParsed" encoding attempts to use program-specific state parsers to return more human-readable and explicit account state data. If "jsonParsed" is requested but a valid mint cannot be found for a particular account, that account will be filtered out from results.
  - (optional) `dataSlice: <object>` - limit the returned account data using the provided `offset: <usize>` and `length: <usize>` fields; only available for "base58", "base64" or "base64+zstd" encodings.

#### النتائج:

The result will be an RpcResponse JSON object with `value` equal to an array of JSON objects, which will contain:

- `pubkey: <string>` - the account Pubkey as base-58 encoded string
- `account: <object>` - a JSON object, with the following sub fields:
  - `lamports: <u64>`, number of lamports assigned to this account, as a u64
  - `owner: <string>`, base-58 encoded Pubkey of the program this account has been assigned to
  - `data: <object>`, Token state data associated with the account, either as encoded binary data or in JSON format `{<program>: <state>}`
  - `executable: <bool>`, boolean indicating if the account contains a program \(and is strictly read-only\)
  - `rentEpoch: <u64>`, the epoch at which this account will next owe rent, as u64

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

Result:

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
                "decimals": 1,
                "uiAmount": 0.1,
                "uiAmountString": "0.1"
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

### getTokenLargestAccounts

Returns the 20 largest accounts of a particular SPL Token type.

#### المُعلمات (parameters):

- `<string>` - Pubkey of token Mint to query, as base-58 encoded string
- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### النتائج:

The result will be an RpcResponse JSON object with `value` equal to an array of JSON objects containing:

- `address: <string>` - the address of the token account
- `amount: <string>` - the raw token account balance without decimals, a string representation of u64
- `decimals: <u8>` - number of base 10 digits to the right of the decimal place
- `uiAmount: <number | null>` - the token account balance, using mint-prescribed decimals **DEPRECATED**
- `uiAmountString: <string>` - the token account balance as a string, using mint-prescribed decimals

#### مثال:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getTokenLargestAccounts", "params": ["3wyAj7Rt1TWVPZVteFJPLa26JmLvdb1CAKEFZm3NY75E"]}
'
```

Result:

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
        "uiAmount": 7.71,
        "uiAmountString": "7.71"
      },
      {
        "address": "BnsywxTcaYeNUtzrPxQUvzAWxfzZe3ZLUJ4wMMuLESnu",
        "amount": "229",
        "decimals": 2,
        "uiAmount": 2.29,
        "uiAmountString": "2.29"
      }
    ]
  },
  "id": 1
}
```

### getTokenSupply

Returns the total supply of an SPL Token type.

#### المُعلمات (parameters):

- `<string>` - Pubkey of token Mint to query, as base-58 encoded string
- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### النتائج:

The result will be an RpcResponse JSON object with `value` equal to a JSON object containing:

- `amount: <string>` - the raw total token supply without decimals, a string representation of u64
- `decimals: <u8>` - number of base 10 digits to the right of the decimal place
- `uiAmount: <number | null>` - the total token supply, using mint-prescribed decimals **DEPRECATED**
- `uiAmountString: <string>` - the total token supply as a string, using mint-prescribed decimals

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
      "amount": "100000",
      "decimals": 2,
      "uiAmount": 1000,
      "uiAmountString": "1000"
    }
  },
  "id": 1
}
```

### getTransaction

Returns transaction details for a confirmed transaction

#### المُعلمات (parameters):

- `<string>` - transaction signature as base-58 encoded string
- `<object>` - (optional) Configuration object containing the following optional fields:
  - (optional) `encoding: <string>` - encoding for each returned Transaction, either "json", "jsonParsed", "base58" (_slow_), "base64". If parameter not provided, the default encoding is "json". "jsonParsed" encoding attempts to use program-specific instruction parsers to return more human-readable and explicit data in the `transaction.message.instructions` list. If "jsonParsed" is requested but a parser cannot be found, the instruction falls back to regular JSON encoding (`accounts`, `data`, and `programIdIndex` fields).
  - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment); "processed" is not supported. If parameter not provided, the default is "finalized".

#### النتائج:

- `<null>` - if transaction is not found or not confirmed
- `<object>` - if transaction is confirmed, an object with the following fields:
  - `slot: <u64>` - the slot this transaction was processed in
  - `transaction: <object|[string,encoding]>` - [Transaction](#transaction-structure) object, either in JSON format or encoded binary data, depending on encoding parameter
  - `blockTime: <i64 | null>` - estimated production time, as Unix timestamp (seconds since the Unix epoch) of when the transaction was processed. null if not available
  - `meta: <object | null>` - transaction status metadata object:
    - `err: <object | null>` - Error if transaction failed, null if transaction succeeded. [TransactionError definitions](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
    - `fee: <u64>` - fee this transaction was charged, as u64 integer
    - `preBalances: <array>` - array of u64 account balances from before the transaction was processed
    - `postBalances: <array>` - array of u64 account balances after the transaction was processed
    - `innerInstructions: <array|undefined>` - List of [inner instructions](#inner-instructions-structure) or omitted if inner instruction recording was not yet enabled during this transaction
    - `preTokenBalances: <array|undefined>` - List of [token balances](#token-balances-structure) from before the transaction was processed or omitted if token balance recording was not yet enabled during this transaction
    - `postTokenBalances: <array|undefined>` - List of [token balances](#token-balances-structure) from after the transaction was processed or omitted if token balance recording was not yet enabled during this transaction
    - `logMessages: <array>` - array of string log messages or omitted if log message recording was not yet enabled during this transaction
    - DEPRECATED: `status: <object>` - Transaction status
      - `"Ok": <null>` - Transaction was successful
      - `"Err": <ERR>` - Transaction failed with TransactionError

#### مثال:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getTransaction",
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
      "postTokenBalances": [],
      "preBalances": [499998937500, 26858640, 1, 1, 1],
      "preTokenBalances": [],
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
  "blockTime": null,
  "id": 1
}
```

#### Example:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getTransaction",
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
      "postTokenBalances": [],
      "preBalances": [499998937500, 26858640, 1, 1, 1],
      "preTokenBalances": [],
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

### getTransactionCount

Returns the current Transaction count from the ledger

#### Parameters:

- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

- `<u64>` - count

#### Example:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getTransactionCount"}
'

```

Result:

```json
{ "jsonrpc": "2.0", "result": 268, "id": 1 }
```

### getVersion

Returns the current solana versions running on the node

#### Parameters:

None

#### Results:

The result field will be a JSON object with the following fields:

- `solana-core`, software version of solana-core
- `feature-set`, unique identifier of the current software's feature set

#### Example:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getVersion"}
'
```

Result:

```json
{ "jsonrpc": "2.0", "result": { "solana-core": "1.7.0" }, "id": 1 }
```

### getVoteAccounts

Returns the account info and associated stake for all the voting accounts in the current bank.

#### Parameters:

- `<object>` - (optional) Configuration object containing the following field:
  - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)
  - (optional) `votePubkey: <string>` - Only return results for this validator vote address (base-58 encoded)

#### Results:

The result field will be a JSON object of `current` and `delinquent` accounts, each containing an array of JSON objects with the following sub fields:

- `votePubkey: <string>` - Vote account address, as base-58 encoded string
- `nodePubkey: <string>` - Validator identity, as base-58 encoded string
- `activatedStake: <u64>` - the stake, in lamports, delegated to this vote account and active in this epoch
- `epochVoteAccount: <bool>` - bool, whether the vote account is staked for this epoch
- `commission: <number>`, percentage (0-100) of rewards payout owed to the vote account
- `lastVote: <u64>` - Most recent slot voted on by this vote account
- `epochCredits: <array>` - History of how many credits earned by the end of each epoch, as an array of arrays containing: `[epoch, credits, previousCredits]`

#### Example:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getVoteAccounts"}
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

#### Example: Restrict results to a single validator vote account

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getVoteAccounts",
    "params": [
      {
        "votePubkey": "3ZT31jkAGhUaw8jsy4bTknwBMP8i4Eueh52By4zXcsVw"
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
    "delinquent": []
  },
  "id": 1
}
```

### minimumLedgerSlot

Returns the lowest slot that the node has information about in its ledger. This value may increase over time if the node is configured to purge older ledger data

#### Parameters:

لا شيء

#### Results:

- `u64` - Minimum ledger slot

#### Example:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"minimumLedgerSlot"}
'

```

النتيجة:

```json
{ "jsonrpc": "2.0", "result": 1234, "id": 1 }
```

### requestAirdrop

Requests an airdrop of lamports to a Pubkey

#### Parameters:

- `<string>` - Pubkey of account to receive lamports, as base-58 encoded string
- `<integer>` - lamports, as a u64
- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment) (used for retrieving blockhash and verifying airdrop success)

#### Results:

- `<string>` - Transaction Signature of airdrop, as base-58 encoded string

#### Example:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"requestAirdrop", "params":["83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri", 1000000000]}
'

```

Result:

```json
{
  "jsonrpc": "2.0",
  "result": "5VERv8NMvzbJMEkV8xnrLkEaWRtSz9CosKDYjCJjBRnbJLgp8uirBgmQpjKhoR4tjF3ZpRzrFmBV6UjKdiSZkQUW",
  "id": 1
}
```

### sendTransaction

Submits a signed transaction to the cluster for processing.

This method does not alter the transaction in any way; it relays the transaction created by clients to the node as-is.

If the node's rpc service receives the transaction, this method immediately succeeds, without waiting for any confirmations. A successful response from this method does not guarantee the transaction is processed or confirmed by the cluster.

While the rpc service will reasonably retry to submit it, the transaction could be rejected if transaction's `recent_blockhash` expires before it lands.

Use [`getSignatureStatuses`](jsonrpc-api.md#getsignaturestatuses) to ensure a transaction is processed and confirmed.

Before submitting, the following preflight checks are performed:

1. يتم التَحَقُّق من توقيعات المُعاملة
2. تتم مُحاكاة المُعاملة في فُتحة (Slot) البنك المُحَدَّدة بواسطة الإلتزام بالإختبار المبدئي (preflight). عند الفشل سيتم إرجاع خطأ. قد تكون فحوصات الإختبار المبدئي (preflight) مُعطلة إذا رغبت في ذلك. من المُستحسن تحديد الإلتزام نفسه وإلتزام الإختبار المبدئي (preflight) لتجنب السلوك المُربك.

The returned signature is the first signature in the transaction, which is used to identify the transaction ([transaction id](../../terminology.md#transanction-id)). This identifier can be easily extracted from the transaction data before submission.

#### Parameters:

- `<string>` - fully-signed Transaction, as encoded string
- `<object>` - (optional) Configuration object containing the following field:
  - `skipPreflight: <bool>` - if true, skip the preflight transaction checks (default: false)
  - `preflightCommitment: <string>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment) level to use for preflight (default: `"finalized"`).
  - `encoding: <string>` - (optional) Encoding used for the transaction data. Either `"base58"` (_slow_, **DEPRECATED**), or `"base64"`. (default: `"base58"`).

#### Results:

- `<string>` - First Transaction Signature embedded in the transaction, as base-58 encoded string ([transaction id](../../terminology.md#transanction-id))

#### Example:

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

Result:

```json
{
  "jsonrpc": "2.0",
  "result": "2id3YC2jK9G5Wo2phDx4gJVAew8DcY5NAojnVuao8rkxwPYPe8cSwE5GzhEgJA2y8fVjDEo6iR6ykBvDxrTQrtpb",
  "id": 1
}
```

### simulateTransaction

Simulate sending a transaction

#### Parameters:

- `<string>` - Transaction, as an encoded string. The transaction must have a valid blockhash, but is not required to be signed.
- `<object>` - (optional) Configuration object containing the following field:
  - `sigVerify: <bool>` - if true the transaction signatures will be verified (default: false)
  - `commitment: <string>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment) level to simulate the transaction at (default: `"finalized"`).
  - `encoding: <string>` - (optional) Encoding used for the transaction data. Either `"base58"` (_slow_, **DEPRECATED**), or `"base64"`. (default: `"base58"`).

#### Results:

An RpcResponse containing a TransactionStatus object The result will be an RpcResponse JSON object with `value` set to a JSON object with the following fields:

- `err: <object | string | null>` - Error if transaction failed, null if transaction succeeded. [TransactionError definitions](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
- `logs: <array | null>` - Array of log messages the transaction instructions output during execution, null if simulation failed before the transaction was able to execute (for example due to an invalid blockhash or signature verification failure)

#### Example:

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

Result:

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

## Subscription Websocket

After connecting to the RPC PubSub websocket at `ws://<ADDRESS>/`:

- Submit subscription requests to the websocket using the methods below
- Multiple subscriptions may be active at once
- Many subscriptions take the optional [`commitment` parameter](jsonrpc-api.md#configuring-state-commitment), defining how finalized a change should be to trigger a notification. For subscriptions, if commitment is unspecified, the default value is `"finalized"`.

### accountSubscribe

Subscribe to an account to receive notifications when the lamports or data for a given account public key changes

#### Parameters:

- `<string>` - account Pubkey, as base-58 encoded string
- `<object>` - (إختياري) تحتوي إعدادات الكائن على الحقول الإختيارية التالية:
  - `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)
  - `encoding: <string>` - encoding for Account data, either "base58" (_slow_), "base64", "base64+zstd" or "jsonParsed". "jsonParsed" encoding attempts to use program-specific state parsers to return more human-readable and explicit account state data. If "jsonParsed" is requested but a parser cannot be found, the field falls back to binary encoding, detectable when the `data` field is type `<string>`.

#### Results:

- `<number>` - Subscription id \(needed to unsubscribe\)

#### Example:

Request:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "accountSubscribe",
  "params": [
    "CM78CPUeXjn8o3yroDHxUtKsZZgoy4GPkPPXfouKNH12",
    {
      "encoding": "base64",
      "commitment": "finalized"
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

Result:

```json
{ "jsonrpc": "2.0", "result": 23784, "id": 1 }
```

#### Notification Format:

Base58 encoding:

```json
{
  "jsonrpc": "2.0",
  "method": "accountNotification",
  "params": {
    "result": {
      "context": {
        "slot": 5199307
      },
      "value": {
        "data": [
          "11116bv5nS2h3y12kD1yUKeMZvGcKLSjQgX6BeV7u1FrjeJcKfsHPXHRDEHrBesJhZyqnnq9qJeUuF7WHxiuLuL5twc38w2TXNLxnDbjmuR",
          "base58"
        ],
        "executable": false,
        "lamports": 33594,
        "owner": "11111111111111111111111111111111",
        "rentEpoch": 635
      }
    },
    "subscription": 23784
  }
}
```

Parsed-JSON encoding:

```json
{
  "jsonrpc": "2.0",
  "method": "accountNotification",
  "params": {
    "result": {
      "context": {
        "slot": 5199307
      },
      "value": {
        "data": {
          "program": "nonce",
          "parsed": {
            "type": "initialized",
            "info": {
              "authority": "Bbqg1M4YVVfbhEzwA9SpC9FhsaG83YMTYoR4a8oTDLX",
              "blockhash": "LUaQTmM7WbMRiATdMMHaRGakPtCkc2GHtH57STKXs6k",
              "feeCalculator": {
                "lamportsPerSignature": 5000
              }
            }
          }
        },
        "executable": false,
        "lamports": 33594,
        "owner": "11111111111111111111111111111111",
        "rentEpoch": 635
      }
    },
    "subscription": 23784
  }
}
```

### accountUnsubscribe

Unsubscribe from account change notifications

#### Parameters:

- `<number>` - id of account Subscription to cancel

#### Results:

- `<bool>` - رسالة نجاح إلغاء الإشتراك

#### Example:

Request:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "accountUnsubscribe", "params": [0] }
```

Result:

```json
{ "jsonrpc": "2.0", "result": true, "id": 1 }
```

### logsSubscribe

Subscribe to transaction logging

#### Parameters:

- `filter: <string>|<object>` - filter criteria for the logs to receive results by account type; currently supported:
  - "all" - subscribe to all transactions except for simple vote transactions
  - "allWithVotes" - subscribe to all transactions including simple vote transactions
  - `{ "mentions": [ <string> ] }` - subscribe to all transactions that mention the provided Pubkey (as base-58 encoded string)
- `<object>` - (إختياري) تحتوي إعدادات الكائن على الحقول الإختيارية التالية:
  - (إختياري) - الإلتزام [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

- `<integer>` - مُعرف الإشتراك \(مطلوب لإلغاء الإشتراك\)

#### Example:

Request:

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
      "commitment": "finalized"
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

Result:

```json
{ "jsonrpc": "2.0", "result": 24040, "id": 1 }
```

#### Notification Format:

Base58 encoding:

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

### logsUnsubscribe

Unsubscribe from transaction logging

#### Parameters:

- `<integer>` - id of subscription to cancel

#### Results:

- `<bool>` - رسالة نجاح إلغاء الإشتراك

#### Example:

Request:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "logsUnsubscribe", "params": [0] }
```

Result:

```json
{ "jsonrpc": "2.0", "result": true, "id": 1 }
```

### programSubscribe

Subscribe to a program to receive notifications when the lamports or data for a given account owned by the program changes

#### Parameters:

- `<string>` - program_id Pubkey, as base-58 encoded string
- `<object>` - (optional) Configuration object containing the following optional fields:
  - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)
  - `encoding: <string>` - encoding for Account data, either "base58" (_slow_), "base64", "base64+zstd" or "jsonParsed". "jsonParsed" encoding attempts to use program-specific state parsers to return more human-readable and explicit account state data. If "jsonParsed" is requested but a parser cannot be found, the field falls back to base64 encoding, detectable when the `data` field is type `<string>`.
  - (optional) `filters: <array>` - filter results using various [filter objects](jsonrpc-api.md#filters); account must meet all filter criteria to be included in results

#### Results:

- `<integer>` - Subscription id \(needed to unsubscribe\)

#### Example:

Request:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "programSubscribe",
  "params": [
    "11111111111111111111111111111111",
    {
      "encoding": "base64",
      "commitment": "finalized"
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

Result:

```json
{ "jsonrpc": "2.0", "result": 24040, "id": 1 }
```

#### Notification Format:

Base58 encoding:

```json
{
  "jsonrpc": "2.0",
  "method": "programNotification",
  "params": {
    "result": {
      "context": {
        "slot": 5208469
      },
      "value": {
        "pubkey": "H4vnBqifaSACnKa7acsxstsY1iV1bvJNxsCY7enrd1hq",
        "account": {
          "data": [
            "11116bv5nS2h3y12kD1yUKeMZvGcKLSjQgX6BeV7u1FrjeJcKfsHPXHRDEHrBesJhZyqnnq9qJeUuF7WHxiuLuL5twc38w2TXNLxnDbjmuR",
            "base58"
          ],
          "executable": false,
          "lamports": 33594,
          "owner": "11111111111111111111111111111111",
          "rentEpoch": 636
        }
      }
    },
    "subscription": 24040
  }
}
```

Parsed-JSON encoding:

```json
{
  "jsonrpc": "2.0",
  "method": "programNotification",
  "params": {
    "result": {
      "context": {
        "slot": 5208469
      },
      "value": {
        "pubkey": "H4vnBqifaSACnKa7acsxstsY1iV1bvJNxsCY7enrd1hq",
        "account": {
          "data": {
            "program": "nonce",
            "parsed": {
              "type": "initialized",
              "info": {
                "authority": "Bbqg1M4YVVfbhEzwA9SpC9FhsaG83YMTYoR4a8oTDLX",
                "blockhash": "LUaQTmM7WbMRiATdMMHaRGakPtCkc2GHtH57STKXs6k",
                "feeCalculator": {
                  "lamportsPerSignature": 5000
                }
              }
            }
          },
          "executable": false,
          "lamports": 33594,
          "owner": "11111111111111111111111111111111",
          "rentEpoch": 636
        }
      }
    },
    "subscription": 24040
  }
}
```

### programUnsubscribe

Unsubscribe from program-owned account change notifications

#### Parameters:

- `<integer>` - id of account Subscription to cancel

#### Results:

- `<bool>` - رسالة نجاح إلغاء الإشتراك

#### Example:

الطلب:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "programUnsubscribe", "params": [0] }
```

النتيجة:

```json
{ "jsonrpc": "2.0", "result": true, "id": 1 }
```

### signatureSubscribe

Subscribe to a transaction signature to receive notification when the transaction is confirmed On `signatureNotification`, the subscription is automatically cancelled

#### Parameters:

- `<string>` - Transaction Signature, as base-58 encoded string
- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

- `integer` - subscription id \(needed to unsubscribe\)

#### Example:

Request:

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
      "commitment": "finalized"
    }
  ]
}
```

Result:

```json
{ "jsonrpc": "2.0", "result": 0, "id": 1 }
```

#### Notification Format:

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

### signatureUnsubscribe

Unsubscribe from signature confirmation notification

#### Parameters:

- `<integer>` - subscription id to cancel

#### Results:

- `<bool>` - unsubscribe success message

#### Example:

Request:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "signatureUnsubscribe", "params": [0] }
```

Result:

```json
{ "jsonrpc": "2.0", "result": true, "id": 1 }
```

### slotSubscribe

Subscribe to receive notification anytime a slot is processed by the validator

#### Parameters:

None

#### Results:

- `integer` - subscription id \(needed to unsubscribe\)

#### Example:

Request:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "slotSubscribe" }
```

Result:

```json
{ "jsonrpc": "2.0", "result": 0, "id": 1 }
```

#### Notification Format:

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

### slotUnsubscribe

Unsubscribe from slot notifications

#### Parameters:

- `<integer>` - subscription id to cancel

#### Results:

- `<bool>` - unsubscribe success message

#### Example:

الطلب:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "slotUnsubscribe", "params": [0] }
```

النتيجة:

```json
{ "jsonrpc": "2.0", "result": true, "id": 1 }
```

### rootSubscribe

Subscribe to receive notification anytime a new root is set by the validator.

#### Parameters:

None

#### Results:

- `integer` - subscription id \(needed to unsubscribe\)

#### Example:

الطلب:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "rootSubscribe" }
```

Result:

```json
{ "jsonrpc": "2.0", "result": 0, "id": 1 }
```

#### Notification Format:

The result is the latest root slot number.

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

### rootUnsubscribe

Unsubscribe from root notifications

#### Parameters:

- `<integer>` - subscription id to cancel

#### Results:

- `<bool>` - unsubscribe success message

#### Example:

Request:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "rootUnsubscribe", "params": [0] }
```

Result:

```json
{ "jsonrpc": "2.0", "result": true, "id": 1 }
```

### voteSubscribe - Unstable, disabled by default

**This subscription is unstable and only available if the validator was started with the `--rpc-pubsub-enable-vote-subscription` flag. The format of this subscription may change in the future**

Subscribe to receive notification anytime a new vote is observed in gossip. These votes are pre-consensus therefore there is no guarantee these votes will enter the ledger.

#### Parameters:

None

#### Results:

- `integer` - subscription id \(needed to unsubscribe\)

#### Example:

Request:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "voteSubscribe" }
```

Result:

```json
{ "jsonrpc": "2.0", "result": 0, "id": 1 }
```

#### Notification Format:

The result is the latest vote, containing its hash, a list of voted slots, and an optional timestamp.

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

### voteUnsubscribe

Unsubscribe from vote notifications

#### Parameters:

- `<integer>` - subscription id to cancel

#### Results:

- `<bool>` - unsubscribe success message

#### Example:

Request:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "voteUnsubscribe", "params": [0] }
```

Response:

```json
{ "jsonrpc": "2.0", "result": true, "id": 1 }
```

## JSON RPC API Deprecated Methods

### getConfirmedBlock

**DEPRECATED: Please use [getBlock](jsonrpc-api.md#getblock) instead** This method is expected to be removed in solana-core v1.8

Returns identity and transaction information about a confirmed block in the ledger

#### Parameters:

- `<u64>` - slot, as u64 integer
- `<object>` - (optional) Configuration object containing the following optional fields:
  - (optional) `encoding: <string>` - encoding for each returned Transaction, either "json", "jsonParsed", "base58" (_slow_), "base64". If parameter not provided, the default encoding is "json". "jsonParsed" encoding attempts to use program-specific instruction parsers to return more human-readable and explicit data in the `transaction.message.instructions` list. If "jsonParsed" is requested but a parser cannot be found, the instruction falls back to regular JSON encoding (`accounts`, `data`, and `programIdIndex` fields).
  - (optional) `transactionDetails: <string>` - level of transaction detail to return, either "full", "signatures", or "none". If parameter not provided, the default detail level is "full".
  - (optional) `rewards: bool` - whether to populate the `rewards` array. If parameter not provided, the default includes rewards.
  - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment); "processed" is not supported. If parameter not provided, the default is "finalized".

#### Results:

The result field will be an object with the following fields:

- `<null>` - if specified block is not confirmed
- `<object>` - if block is confirmed, an object with the following fields:
  - `blockhash: <string>` - the blockhash of this block, as base-58 encoded string
  - `previousBlockhash: <string>` - the blockhash of this block's parent, as base-58 encoded string; if the parent block is not available due to ledger cleanup, this field will return "11111111111111111111111111111111"
  - `parentSlot: <u64>` - the slot index of this block's parent
  - `transactions: <array>` - present if "full" transaction details are requested; an array of JSON objects containing:
    - `transaction: <object|[string,encoding]>` - [Transaction](#transaction-structure) object, either in JSON format or encoded binary data, depending on encoding parameter
    - `meta: <object>` - transaction status metadata object, containing `null` or:
      - `err: <object | null>` - Error if transaction failed, null if transaction succeeded. [TransactionError definitions](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
      - `fee: <u64>` - fee this transaction was charged, as u64 integer
      - `preBalances: <array>` - array of u64 account balances from before the transaction was processed
      - `postBalances: <array>` - array of u64 account balances after the transaction was processed
      - `innerInstructions: <array|undefined>` - List of [inner instructions](#inner-instructions-structure) or omitted if inner instruction recording was not yet enabled during this transaction
      - `preTokenBalances: <array|undefined>` - List of [token balances](#token-balances-structure) from before the transaction was processed or omitted if token balance recording was not yet enabled during this transaction
      - `postTokenBalances: <array|undefined>` - List of [token balances](#token-balances-structure) from after the transaction was processed or omitted if token balance recording was not yet enabled during this transaction
      - `logMessages: <array>` - array of string log messages or omitted if log message recording was not yet enabled during this transaction
      - DEPRECATED: `status: <object>` - Transaction status
        - `"Ok": <null>` - Transaction was successful
        - `"Err": <ERR>` - Transaction failed with TransactionError
  - `signatures: <array>` - present if "signatures" are requested for transaction details; an array of signatures strings, corresponding to the transaction order in the block
  - `rewards: <array>` - present if rewards are requested; an array of JSON objects containing:
    - `pubkey: <string>` - The public key, as base-58 encoded string, of the account that received the reward
    - `lamports: <i64>`- number of reward lamports credited or debited by the account, as a i64
    - `postBalance: <u64>` - account balance in lamports after the reward was applied
    - `rewardType: <string|undefined>` - type of reward: "fee", "rent", "voting", "staking"
  - `blockTime: <i64 | null>` - estimated production time, as Unix timestamp (seconds since the Unix epoch). null if not available

#### Example:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc": "2.0","id":1,"method":"getConfirmedBlock","params":[430, {"encoding": "json","transactionDetails":"full","rewards":false}]}
'
```

Result:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "blockTime": null,
    "blockhash": "3Eq21vXNB5s86c62bVuUfTeaMif1N2kUqRPBmGRJhyTA",
    "parentSlot": 429,
    "previousBlockhash": "mfcyqEXB3DnHXki6KjjmZck6YjmZLvpAByy2fj4nh6B",
    "transactions": [
      {
        "meta": {
          "err": null,
          "fee": 5000,
          "innerInstructions": [],
          "logMessages": [],
          "postBalances": [499998932500, 26858640, 1, 1, 1],
          "postTokenBalances": [],
          "preBalances": [499998937500, 26858640, 1, 1, 1],
          "preTokenBalances": [],
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

#### Example:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc": "2.0","id":1,"method":"getConfirmedBlock","params":[430, "base64"]}
'
```

Result:

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
          "postTokenBalances": [],
          "preBalances": [499998937500, 26858640, 1, 1, 1],
          "preTokenBalances": [],
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

For more details on returned data: [Transaction Structure](jsonrpc-api.md#transactionstructure) [Inner Instructions Structure](jsonrpc-api.md#innerinstructionsstructure) [Token Balances Structure](jsonrpc-api.md#tokenbalancesstructure)

### getConfirmedBlocks

**DEPRECATED: Please use [getBlocks](jsonrpc-api.md#getblocks) instead** This method is expected to be removed in solana-core v1.8

Returns a list of confirmed blocks between two slots

#### Parameters:

- `<u64>` - start_slot, as u64 integer
- `<u64>` - (optional) end_slot, as u64 integer
- (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment); "processed" is not supported. If parameter not provided, the default is "finalized".

#### Results:

The result field will be an array of u64 integers listing confirmed blocks between `start_slot` and either `end_slot`, if provided, or latest confirmed block, inclusive. Max range allowed is 500,000 slots.

#### Example:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc": "2.0","id":1,"method":"getConfirmedBlocks","params":[5, 10]}
'
```

Result:

```json
{ "jsonrpc": "2.0", "result": [5, 6, 7, 8, 9, 10], "id": 1 }
```

### getConfirmedBlocksWithLimit

**DEPRECATED: Please use [getBlocksWithLimit](jsonrpc-api.md#getblockswithlimit) instead** This method is expected to be removed in solana-core v1.8

Returns a list of confirmed blocks starting at the given slot

#### Parameters:

- `<u64>` - start_slot, as u64 integer
- `<u64>` - limit, as u64 integer
- (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment); "processed" is not supported. If parameter not provided, the default is "finalized".

#### Results:

The result field will be an array of u64 integers listing confirmed blocks starting at `start_slot` for up to `limit` blocks, inclusive.

#### Example:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc": "2.0","id":1,"method":"getConfirmedBlocksWithLimit","params":[5, 3]}
'
```

Result:

```json
{ "jsonrpc": "2.0", "result": [5, 6, 7], "id": 1 }
```

### getConfirmedSignaturesForAddress2

**DEPRECATED: Please use [getSignaturesForAddress](jsonrpc-api.md#getsignaturesforaddress) instead** This method is expected to be removed in solana-core v1.8

Returns confirmed signatures for transactions involving an address backwards in time from the provided signature or most recent confirmed block

#### Parameters:

- `<string>` - account address as base-58 encoded string
- `<object>` - (optional) Configuration object containing the following fields:
  - `limit: <number>` - (optional) maximum transaction signatures to return (between 1 and 1,000, default: 1,000).
  - `before: <string>` - (optional) start searching backwards from this transaction signature. If not provided the search starts from the top of the highest max confirmed block.
  - `until: <string>` - (optional) search until this transaction signature, if found before limit reached.
  - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment); "processed" is not supported. If parameter not provided, the default is "finalized".

#### Results:

The result field will be an array of transaction signature information, ordered from newest to oldest transaction:

- `<object>`
  - `signature: <string>` - transaction signature as base-58 encoded string
  - `slot: <u64>` - The slot that contains the block with the transaction
  - `err: <object | null>` - Error if transaction failed, null if transaction succeeded. [TransactionError definitions](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
  - `memo: <string |null>` - Memo associated with the transaction, null if no memo is present
  - `blockTime: <i64 | null>` - estimated production time, as Unix timestamp (seconds since the Unix epoch) of when transaction was processed. null if not available.

#### Example:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getConfirmedSignaturesForAddress2",
    "params": [
      "Vote111111111111111111111111111111111111111",
      {
        "limit": 1
      }
    ]
  }
'
```

Result:

```json
{
  "jsonrpc": "2.0",
  "result": [
    {
      "err": null,
      "memo": null,
      "signature": "5h6xBEauJ3PK6SWCZ1PGjBvj8vDdWG3KpwATGy1ARAXFSDwt8GFXM7W5Ncn16wmqokgpiKRLuS83KUxyZyv2sUYv",
      "slot": 114,
      "blockTime": null
    }
  ],
  "id": 1
}
```

### getConfirmedTransaction

**DEPRECATED: Please use [getTransaction](jsonrpc-api.md#gettransaction) instead** This method is expected to be removed in solana-core v1.8

Returns transaction details for a confirmed transaction

#### Parameters:

- `<string>` - transaction signature as base-58 encoded string
- `<object>` - (optional) Configuration object containing the following optional fields:
  - (optional) `encoding: <string>` - encoding for each returned Transaction, either "json", "jsonParsed", "base58" (_slow_), "base64". If parameter not provided, the default encoding is "json". "jsonParsed" encoding attempts to use program-specific instruction parsers to return more human-readable and explicit data in the `transaction.message.instructions` list. If "jsonParsed" is requested but a parser cannot be found, the instruction falls back to regular JSON encoding (`accounts`, `data`, and `programIdIndex` fields).
  - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment); "processed" is not supported. If parameter not provided, the default is "finalized".

#### Results:

- `<null>` - if transaction is not found or not confirmed
- `<object>` - if transaction is confirmed, an object with the following fields:
  - `slot: <u64>` - the slot this transaction was processed in
  - `transaction: <object|[string,encoding]>` - [Transaction](#transaction-structure) object, either in JSON format or encoded binary data, depending on encoding parameter
  - `blockTime: <i64 | null>` - estimated production time, as Unix timestamp (seconds since the Unix epoch) of when the transaction was processed. null if not available
  - `meta: <object | null>` - transaction status metadata object:
    - `err: <object | null>` - Error if transaction failed, null if transaction succeeded. [TransactionError definitions](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
    - `fee: <u64>` - fee this transaction was charged, as u64 integer
    - `preBalances: <array>` - array of u64 account balances from before the transaction was processed
    - `postBalances: <array>` - array of u64 account balances after the transaction was processed
    - `innerInstructions: <array|undefined>` - List of [inner instructions](#inner-instructions-structure) or omitted if inner instruction recording was not yet enabled during this transaction
    - `preTokenBalances: <array|undefined>` - List of [token balances](#token-balances-structure) from before the transaction was processed or omitted if token balance recording was not yet enabled during this transaction
    - `postTokenBalances: <array|undefined>` - List of [token balances](#token-balances-structure) from after the transaction was processed or omitted if token balance recording was not yet enabled during this transaction
    - `logMessages: <array>` - array of string log messages or omitted if log message recording was not yet enabled during this transaction
    - DEPRECATED: `status: <object>` - Transaction status
      - `"Ok": <null>` - Transaction was successful
      - `"Err": <ERR>` - Transaction failed with TransactionError

#### Example:

Request:

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

Result:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "meta": {
      "err": null,
      "fee": 5000,
      "innerInstructions": [],
      "postBalances": [499998932500, 26858640, 1, 1, 1],
      "postTokenBalances": [],
      "preBalances": [499998937500, 26858640, 1, 1, 1],
      "preTokenBalances": [],
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
  "blockTime": null,
  "id": 1
}
```

#### Example:

Request:

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

Result:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "meta": {
      "err": null,
      "fee": 5000,
      "innerInstructions": [],
      "postBalances": [499998932500, 26858640, 1, 1, 1],
      "postTokenBalances": [],
      "preBalances": [499998937500, 26858640, 1, 1, 1],
      "preTokenBalances": [],
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
