declare module "tweetnacl" {
  declare type KeyPair = {
    publicKey: Buffer;
    secretKey: Buffer;
  };

  declare type KeypairFunc = {
    (): KeyPair,
    fromSecretKey(secretKey: Buffer): KeyPair,
    fromSeed(seed: Uint8Array): KeyPair,

  };
  declare module.exports: {
    sign: {
      keyPair: KeypairFunc;
      detached(text: Buffer, secretKey: Buffer): Buffer;
    };
  };
}

