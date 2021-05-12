---
title: مراقبة المُدقّق (Monitoring a Validator)
---

## التحقق من القيل والقال (Check Gossip)

تأكد من أن عنوان IP و مفتاح الهوية **identity pubkey** من المُدقّق (validator) الخاص بك مرئي في شبكة القيل والقال (gossip) عن طريق تشغيل الأمر البرمجي:

```bash
solana-gossip spy --entrypoint devnet.solana.com:8001
```

## التحقق من رصيدك (Check Your Balance)

يجب أن ينخفض رصيد حسابك بمقدار رسوم المُعاملة حيث يقوم المُدقّق (validator) الخاص بك بتقديم الأصوات، ويزيد بعد العمل كقائد (leader). تمرير الـ `--lamports` يجب أن تُراقب بتفصيل أدق:

```bash
solana balance --lamports
```

## تحقق من نشاط التصويت (Check Vote Activity)

يعرض الأمر `solana vote-account` نشاط التصويت الأخير من المُدقّق (validator) الخاص بك:

```bash
solana vote-account ~/vote-account-keypair.json
```

## الحصول على معلومات المجموعة (Get Cluster Info)

تُوجد عدة نقاط نهاية (endpoints) مُفيدة لـ JSON-RPC لمُراقبة المُدقّق (validator) الخاص بك في المجموعة (cluster)، بالإضافة إلى صحة المجموعة (cluster):

```bash
# Similar to solana-gossip, you should see your validator in the list of cluster nodes
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getClusterNodes"}' http://devnet.solana.com
# If your validator is properly voting, it should appear in the list of `current` vote accounts. If staked, `stake` should be > 0
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getVoteAccounts"}' http://devnet.solana.com
# Returns the current leader schedule
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getLeaderSchedule"}' http://devnet.solana.com
# Returns info about the current epoch. slotIndex should progress on subsequent calls.
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getEpochInfo"}' http://devnet.solana.com
```
