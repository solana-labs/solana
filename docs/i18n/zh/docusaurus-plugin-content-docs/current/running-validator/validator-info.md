---
title: 发布验证者信息
---

您可以将验证者信息发布到链上，以使其对其他用户公开可见。

## 运行solana validator-info

运行solana CLI来获取一个验证者信息帐户：

```bash
solana validator-info publish --keypair ~/validator-keypair.json <VALIDATOR_INFO_ARGS> <VALIDATOR_NAME>
```

关于VALIDATOR_INFO_ARGS可选字段的详细信息：

```bash
solana validator-info publish --help
```

## 示例命令

发布命令示例：

```bash
solana validator-info publish "Elvis Validator" -n elvis -w "https://elvis-validates.com"
```

示例查询命令：

```bash
solana validator-info get
```

输出为

```text
Validator info from 8WdJvDz6obhADdxpGCiJKZsDYwTLNEDFizayqziDc9ah
  Validator pubkey: 6dMH3u76qZ7XG4bVboVRnBHR2FfrxEqTTTyj4xmyDMWo
  Info: {"keybaseUsername":"elvis","name":"Elvis Validator","website":"https://elvis-validates.com"}
```

## 密钥库

包括Keybase用户名，客户端应用程序\(例如Solana Network Explorer \) 可以自动引入您的验证节点公共配置文件，包括密码证明，品牌标识等。 要将验证器公钥与Keybase连接：

1. 加入[https://keybase.io/](https://keybase.io/)并填写您的验证节点个人资料
2. 将您的验证节点**身份pubkey**添加到Keybase：

   - 在本地计算机上创建一个名为`validator-<PUBKEY>`的空文件。
   - 在“密钥库”中，导航到“文件”，然后将您的pubkey文件上传到

     公用文件夹中的`solana`子目录：`/keybase/public/<KEYBASE_USERNAME>/solana`

   - 要检查您的公钥，请确保您可以成功浏览到

     `https://keybase.pub/<KEYBASE_USERNAME>/solana/validator-<PUBKEY>`

3. 使用Keybase用户名添加或更新您的`solana Validator-info`。 然后

   CLI将验证`validator-<PUBKEY>`文件
