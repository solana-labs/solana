---
title: إدارة حِسابات إثبات الحِصَّة أو التَّحْصِيص (Manage Stake Accounts)
---

إذا كنت ترغب في تفويض الحِصَّة إلى إلى أكثر من مُدقّق (validator)، فستحتاج إلى إنشاء حساب إثبات حِصَّة أو تَّحْصِيص مُنفصلة لكل منها. إذا إتبعت الإتفاقية لإنشاء حساب إثبات حِصَّة أو تَّحْصِيص (stake account) أول على أساس "0" ، والثاني عند "1"، و الثالث عند "2"، وهكذا، سيُتيح لك الأداة `solana-stake-accounts` العمل على جميع الحسابات بإستدعاءات واحدة. يُمكنك إستخدامه لتعداد أرصدة جميع الحسابات أو نقل الحسابات إلى محفظة جديدة أو تعيين صلاحيات جديدة.

## طريقة الإستخدام

### إنشاء حِساب إثبات الحِصَّة أو التَّحْصِيص (Create a stake account)

إنشاء وتمويل حِساب إثبات حِصَّة أو تحْصِيص (stake account) مُشتق من المفتاح العمومي لسُلطة إثبات الحِصَّة أو التَّحْصِيص (Stake authority public key):

```bash
solana-stake-accounts new <FUNDING_KEYPAIR> <BASE_KEYPAIR> <AMOUNT> \
    --stake-authority <PUBKEY> --withdraw-authority <PUBKEY> \
    --fee-payer <KEYPAIR>
```

### عد الحسابات

عد عدد الحسابات المُشتقة:

```bash
solana-stake-accounts count <BASE_PUBKEY>
```

### الحصول على أرصدة حِساب إثبات الحِصَّة أو التَّحْصِيص (Get stake account balances)

مجموع رصيد حِسابات إثبات الحِصَّة أو التَّحْصِيص (Stake accounts) المُشتقة:

```bash
solana-stake-accounts balance <BASE_PUBKEY> --num-accounts <NUMBER>
```

### الحصول على عناوين حِساب إثبات الحِصَّة أو التَّحْصِيص (Get stake account addresses)

قائمة عنوان كل حِساب إثبات حِصَّة أو تحْصِيص مُشتق من المفتاح العمومي (public key) المُعين:

```bash
solana-stake-accounts addresses <BASE_PUBKEY> --num-accounts <NUMBER>
```

### تعيين صلاحيات جديدة (Set new authorities)

تعيين صلاحيات جديدة على كل حساب من حِسابات إثبات الحِصَّة أو التَّحْصِيص (Stake accounts) المُشتقة:

```bash
solana-stake-accounts authorize <BASE_PUBKEY> \
    --stake-authority <KEYPAIR> --withdraw-authority <KEYPAIR> \
    --new-stake-authority <PUBKEY> --new-withdraw-authority <PUBKEY> \
    --num-accounts <NUMBER> --fee-payer <KEYPAIR>
```

### نقل حِسابات إثبات الحِصَّة أو التَّحْصِيص (Relocate stake accounts)

نقل حِسابات إثبات الحِصَّة أو التَّحْصِيص (Stake accounts):

```bash
solana-stake-accounts rebase <BASE_PUBKEY> <NEW_BASE_KEYPAIR> \
    --stake-authority <KEYPAIR> --num-accounts <NUMBER> \
    --fee-payer <KEYPAIR>
```

لتغيير العنوان ذريا وإعطاء الإذن لكل حساب حِساب إثبات حِصَّة أو تحْصِيص، إستخدم أمر 'move':

```bash
solana-stake-accounts move <BASE_PUBKEY> <NEW_BASE_KEYPAIR> \
    --stake-authority <KEYPAIR> --withdraw-authority <KEYPAIR> \
    --new-stake-authority <PUBKEY> --new-withdraw-authority <PUBKEY> \
    --num-accounts <NUMBER> --fee-payer <KEYPAIR>
```
