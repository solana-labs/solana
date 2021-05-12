---
title: 创建验证节点公钥
---

您需要先注册才能参加到网络中。 请查看 [注册信息](../registration/how-to-register.md)。

为了获得 SOL 奖励，您需要在keybase.io帐户下发布验证者的身份公共密钥。

## **生成密钥对**

1. 如果还没有密钥对，请运行以下命令来为验证节点生成一个：

   ```bash
     solana-keygen new -o ~/validator-keypair.json
   ```

2. 现在可以运行以下命令查看身份公共密钥：

   ```bash
     solana-keygen pubkey ~/validator-keypair.json
   ```

> 注意：“validator-keypair.json”文件也是您的 \(ed25519\) 私钥。

验证节点身份密钥独特识别了您在网络中的验证节点。 **备份此信息至关重要。**

如果您不备份此信息，那么如果您无法访问验证节点的话，将无法对其进行恢复。 如果发生这种情况，您将失去SOL TOO的奖励。

要备份您的验证节点识别密钥， **请备份您的"validator-keypair.json" 文件到一个安全位置。**

## 将您的Solana公钥链接到Keybase帐户

您必须将Solana pubkey链接到Keybase.io帐户。 以下说明介绍了如何通过在服务器上安装Keybase来执行此操作。

1. 在您的机器上安装[Keybase](https://keybase.io/download)。
2. 登录到服务器上的Keybase帐户。 如果您还没有Keybase帐户，请先创建一个。 以下是基本的[Keybase CLI命令列表](https://keybase.io/docs/command_line/basics)。
3. 在公用文件夹中创建一个Solana目录：`mkdir /keybase/public/<KEYBASE_USERNAME>/solana`
4. 在Keybase公共文件夹中按以下格式创建一个空文件，来发布验证者的身份公共密钥：`/keybase/public/<KEYBASE_USERNAME>/solana/validator-<BASE58_PUBKEY>`。 例如：

   ```bash
     touch /keybase/public/<KEYBASE_USERNAME>/solana/validator-<BASE58_PUBKEY>
   ```

5. 要检查公钥是否已成功发布，请确保您在 `https://keybase.pub/<KEYBASE_USERNAME>/solana/validator-<BASE58_PUBKEY>` 看到它。
