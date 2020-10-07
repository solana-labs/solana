import { getWASM, waitReady } from './../wrapper';

export interface KeyPair {
  publicKey: Uint8Array;
  secretKey: Uint8Array;
}

// wraps result from WASM to make it backward compatible with tweetnacl
function wrapKeyPair(pair: {
  publicKey: number[];
  secretKey: number[];
}): KeyPair {
  return {
    publicKey: Uint8Array.from(pair.publicKey),
    secretKey: Uint8Array.from(pair.secretKey),
  };
}

export async function generate(): Promise<KeyPair> {
  await waitReady();
  return wrapKeyPair(getWASM().generateKeyPair());
}

export async function fromSeed(secretKey: Uint8Array): Promise<KeyPair> {
  await waitReady();
  return wrapKeyPair(getWASM().keyPairFromSeed(secretKey));
}

export async function fromSecretKey(secretKey: Uint8Array): Promise<KeyPair> {
  await waitReady();
  return wrapKeyPair(getWASM().keyPairFromSecretKey(secretKey));
}
