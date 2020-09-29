import { getWASM, ensureReady } from "./../wrapper";

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

export function generate(): KeyPair {
  ensureReady();
  return wrapKeyPair(getWASM().generateKeyPair());
}

export function fromSeed(secretKey: Uint8Array): KeyPair {
  ensureReady();
  return wrapKeyPair(getWASM().keyPairFromSeed(secretKey));
}

export function fromSecretKey(secretKey: Uint8Array): KeyPair {
  ensureReady();
  return wrapKeyPair(getWASM().keyPairFromSecretKey(secretKey));
}
