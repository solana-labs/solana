
[ejson](https://github.com/Shopify/ejson) and
[ejson2env](https://github.com/Shopify/ejson2env) are used to manage access
tokens and other secrets required for CI.

#### Setup
```bash
$ sudo gem install ejson ejson2env
```

then obtain the necessary keypair and place it in `/opt/ejson/keys/`.

#### Usage
Run the following command to decrypt the secrets into the environment:
```bash
eval $(ejson2env secrets.ejson)
```

#### Managing secrets.ejson
To decrypt `secrets.ejson` for modification, run:
```bash
$ ejson decrypt secrets.ejson
```

Edit, then run the following to re-encrypt the file **BEFORE COMMITING YOUR
CHANGES**:
```bash
$ ejson encrypt secrets.ejson
```

