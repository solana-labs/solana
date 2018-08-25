declare module "bs58" {
  declare module.exports: {
    encode(input: Buffer): string;
    decode(input: string): Buffer;
  };
}
