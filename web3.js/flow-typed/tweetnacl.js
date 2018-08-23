declare module "tweetnacl" {
  declare type KeyPair = {
    publicKey: Buffer;
    secretKey: Buffer;
  };

  declare module.exports: {
    sign: {
      keyPair(): KeyPair;
    };
  };
}

