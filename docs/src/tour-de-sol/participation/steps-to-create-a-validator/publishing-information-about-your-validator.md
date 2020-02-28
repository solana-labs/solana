# Publish information about your validator

See [here](../../../running-validator/validator-info.md) for background:

Example publish command:

```bash
solana validator-info publish "Elvis Validator" -n elvis -w "https://elvis-validates.com"
```

Example query command:

```bash
solana validator-info get
```

which outputs

```text
Validator info from 8WdJvDz6obhADdxpGCiJKZsDYwTLNEDFizayqziDc9ah
  Validator pubkey: 6dMH3u76qZ7XG4bVboVRnBHR2FfrxEqTTTyj4xmyDMWo
  Info: {"keybaseUsername":"elvis","name":"Elvis Validator","website":"https://elvis-validates.com"}
```
