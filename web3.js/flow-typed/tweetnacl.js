declare module "tweetnacl" {
  declare type KeyPair = {
    publicKey: Buffer;
    secretKey: Buffer;
  };

  declare module.exports: {
    sign: {
      keyPair(): KeyPair;
      detached(text: Buffer, secretKey: Buffer): Buffer;
    };
  };
}

