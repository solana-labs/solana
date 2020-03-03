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

  declare type DetachedFunc = {
    (text: Buffer, secretKey: Buffer): Buffer,
    verify(message: Buffer, signature: Buffer|null, publicKey: Buffer): bool,
  };

  declare module.exports: {
    sign: {
      keyPair: KeypairFunc;
      detached: DetachedFunc;
    };
  };
}
