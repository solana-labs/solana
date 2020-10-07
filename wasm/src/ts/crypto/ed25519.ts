import {getWASM, ensureReady} from './../wrapper';
export * as keypair from './keypair';

export function sign(
  pubkey: Uint8Array,
  seckey: Uint8Array,
  message: Uint8Array,
): Uint8Array {
  ensureReady();
  return getWASM().signEd25519(pubkey, seckey, message);
}

export function verify(
  pubkey: Uint8Array,
  signature: Uint8Array,
  data: Uint8Array,
): boolean {
  ensureReady();
  return getWASM().verifyEd25519(pubkey, signature, data);
}

export function isOnCurve(pubkey: Uint8Array): boolean {
  ensureReady();
  return getWASM().isOnCurveEd25519(pubkey);
}
