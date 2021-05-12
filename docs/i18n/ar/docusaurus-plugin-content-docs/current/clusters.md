---
title: مجموعات Solana
---

تحافظ Solana على العديد من المجموعات المختلفة لأغراض مختلفة.

قبل البدء، تأكد من أنك قمت أولًا [installed the Solana command line tools](cli/install-solana-cli-tools.md)

تصفح:

- [http://explorer.solana.com/](https://explorer.solana.com/).
- [http://solanabeach.io/](http://solanabeach.io/).

## شبكة المُطوِّرين (Devnet)

- تعد شبكة المطورين الساحة التي يجتمع فيها أي شخص يريد أن يجعل من Solana اختبار قيادة، كمستخدم، أو حامل رموز، أو مطور تطبيقات، أو مدقق.
- لذا فإن على مطوري التطبيقات أن يكون هدفهم شبكة المطورين.
- وكذلك ينبغي للمدققين المحتملين أن يكون هدفهم الأول شبكة المطورين.
- الاختلافات الجوهرية بين شبكة المطورين والشبكة التجريبية الرئيسية:
  - تعد رموز شبكة المطورين غير حقبقية **not real**
  - وتشمل شبكة المطورين صنبور التوزيع الحر لاختبار التطبيقات
  - تخضع شبكة المطورين لإعادة تعيين دفتر الأستاذ
  - وعادة ما تقوم شبكة المطورين بتشغيل إصدار برنامج أحدث من الشبكة التجريبية الرئيسية
- نقطة دخول القيل والقال لشبكة المطورين: `entrypoint.devnet.solana.com:8001`
- متغير بيئة القياسات لشبكة المطورين:

```bash
export SOLANA_METRICS_CONFIG="host=https://metrics.solana.com:8086,db=devnet,u=scratch_writer,p=topsecret"
```

- رابط عنوان RPC لشبكة المطورين: `https://devnet.solana.com`

##### مثال تأكيد سطر أوامر `solana`

```bash
solana config set --url https://devnet.solana.com
```

##### مثال سطر أوامر `solana-validator`

```bash
$ solana-validator \
    --identity ~/validator-keypair.json \
    --vote-account ~/vote-account-keypair.json \
    --trusted-validator dv1LfzJvDF7S1fBKpFgKoKXK5yoSosmkAdfbxBo1GqJ \
    --no-untrusted-rpc \
    --ledger ~/validator-ledger \
    --rpc-port 8899 \
    --dynamic-port-range 8000-8010 \
    --entrypoint entrypoint.devnet.solana.com:8001 \
    --expected-genesis-hash EtWTRABZaYq6iMfeYKouRu166VU2xqa1wcaWoxPkrZBG \
    --wal-recovery-mode skip_any_corrupted_record \
    --limit-ledger-size
```

يتم تشغيل المدققين الموثوقين `--trusted-validator` من قبل Solana

## الشبكة التجريبية (Testnet)

- الشبكة التجريبية هي الموقع الذي نقوم فيه بالتأكيد على تجريب خصائص آخر إصدار على مجموعة متصلة، ومرتكزة بشكل خاص على أداء الشبكة واستقرارها وسلوك المدقق.
- وتعمل مبادرة [Tour de SOL](tour-de-sol.md) على الشبكة التجريبية حيث نقوم بتحفيز السلوك الخبيث والهجمات على الشبكة للمساعدة في إيجاد ثغرات أو نقاط ضعف.
- تعد رموز شبكة المطورين غير حقيقية **not real**
- تخضع الشبكة التجريبية لإعادة تعيين دفتر الأستاذ.
- تشمل الشبكة التجريبية على صنبور التوزيع الحر لاختبار التطبيقات
- وعادة ما تقوم الشبكة التجريبية بتشغيل إصدار برنامج أحدث شبكة المطورين والشبكة التجريبية الرئيسية
- نقطة دخول القيل والقال لشبكة المطورين: `entrypoint.testnet.solana.com:8001`
- متغير بيئة القياسات للشبكة التجريبية:

```bash
export SOLANA_METRICS_CONFIG="host=https://metrics.solana.com:8086,db=tds,u=testnet_write,p=c4fa841aa918bf8274e3e2a44d77568d9861b3ea"
```

- رابط عنوان RPC للشبكة التجريبية: `https://testnet.solana.com`

##### مثال تأكيد سطر أوامر `solana`

```bash
solana config set --url https://testnet.solana.com
```

##### مثال سطر أوامر `solana-validator`

```bash
$ solana-validator \
    --identity ~/validator-keypair.json \
    --vote-account ~/vote-account-keypair.json \
    --trusted-validator 5D1fNXzvv5NjV1ysLjirC4WY92RNsVH18vjmcszZd8on \
    --trusted-validator ta1Uvfb7W5BRPrdGnhP9RmeCGKzBySGM1hTE4rBRy6T \
    --trusted-validator Ft5fbkqNa76vnsjYNwjDZUXoTWpP7VYm3mtsaQckQADN \
    --trusted-validator 9QxCLckBiJc783jnMvXZubK4wH86Eqqvashtrwvcsgkv \
    --no-untrusted-rpc \
    --ledger ~/validator-ledger \
    --rpc-port 8899 \
    --dynamic-port-range 8000-8010 \
    --entrypoint entrypoint.testnet.solana.com:8001 \
    --expected-genesis-hash 4uhcVJyU9pJkvQyS88uRDiswHXSCkY3zQawwpjk2NsNY \
    --wal-recovery-mode skip_any_corrupted_record \
    --limit-ledger-size
```

هوية `--الموثوق به`s هي:

- `5D1fNXzvv5NjV1ysLjirC4WY92RNsVH18vjmcszZd8on` - testnet.solana.com (Solana)
- `ta1Uvfb7W5BRPrdGnhP9RmeCGKzBySGM1hTE4rBRy6T` - كسر عقدة RPC (Solana)
- `Ft5fbkqNa76vnsjYNwjDZUXoTWpP7VYm3mtsaQckQADN` - Certus One
- `9QxCLckBiJc783jnMvXZubK4wH86Eqqvashtrwvcsgkv` - Algo|Stake

## الشبكة التجريبية الرئيسية (mainnet beta)

هي عبارة عن مجموعة ثابتة لا تحتاج إلى إذن لحاملي الرموز الأوائل وشركاء الانطلاق. حاليًا، تعتبر المكافآت والتضخم غير مفعلة.

- الرموز التي تم إصدارها على الشبكة الرئيسية التجريبية **real** SOL
- إذا قمت بدفع المال لشراء إنشاء رموز من أجلك، مثلًا من خلال مزاد CoinList، فبالإمكان تحويل هذه الرموز على الشبكة الرئيسية التجريبية.
  - ملاحظة: إذا كنت تستخدم محفظة لا تحتوي على سطر أوامر مثل [Solflare](wallet-guide/solflare.md)، فستكون المحفظة متصلة دائمًا بالشبكة الرئيسية التجريبية.
- نقطة دخول القيل والقال للشبكة الرئيسية التجريبية: `entrypoint.mainnet-beta.solana.com:8001`
- متغير بيئة القياسات للشبكة الرئيسية التجريبية:

```bash
export SOLANA_METRICS_CONFIG="host=https://metrics.solana.com:8086,db=mainnet-beta,u=mainnet-beta_write,p=password"
```

- رابط عنوان RPC للشبكة الرئيسية التجريبية: `https://api.mainnet-beta.solana.com`

##### مثال تأكيد سطر أوامر `solana`

```bash
solana config set --url https://api.mainnet-beta.solana.com
```

##### مثال سطر أوامر `solana-validator`

```bash
$ solana-validator \
    --identity ~/validator-keypair.json \
    --vote-account ~/vote-account-keypair.json \
    --trusted-validator 7Np41oeYqPefeNQEHSv1UDhYrehxin3NStELsSKCT4K2 \
    --trusted-validator GdnSyH3YtwcxFvQrVVJMm1JhTS4QVX7MFsX56uJLUfiZ \
    --trusted-validator DE1bawNcRJB9rVm3buyMVfr8mBEoyyu73NBovf2oXJsJ \
    --trusted-validator CakcnaRDHka2gXyfbEd2d3xsvkJkqsLw2akB3zsN1D2S \
    --no-untrusted-rpc \
    --ledger ~/validator-ledger \
    --rpc-port 8899 \
    --private-rpc \
    --dynamic-port-range 8000-8010 \
    --entrypoint entrypoint.mainnet-beta.solana.com:8001 \
    --entrypoint entrypoint2.mainnet-beta.solana.com:8001 \
    --entrypoint entrypoint3.mainnet-beta.solana.com:8001 \
    --entrypoint entrypoint4.mainnet-beta.solana.com:8001 \
    --entrypoint entrypoint5.mainnet-beta.solana.com:8001 \
    --expected-genesis-hash 5eykt4UsFv8P8NJdTREpY1vzqKqZKvdpKuc147dw2N9d \
    --wal-recovery-mode skip_any_corrupted_record \
    --limit-ledger-size
```

يتم تشغيل المدققين الموثوقين الأربعة `--trusted-validator` من قبل Solana
